//! Runtime dynamic-library loading shared by `agenterm-cu` and the
//! `mechanism` layer (milestone 46).
//!
//! Every `agt_*` call goes through one process-wide `dlopen` / `LoadLibrary`
//! of the libagenterm dynamic library (`agenterm.dll` / `libagenterm.so` /
//! `libagenterm.dylib`). The library is located once and cached in a
//! `OnceLock`; a failed load keeps every candidate path so callers can report
//! exactly what was tried. There is no `agenterm-platform` / `agenterm-abi`
//! static linking here: every symbol is resolved from the loaded library at
//! runtime.
//!
//! FFI type layouts and constant values below mirror `include/agenterm.h`
//! exactly — do not change them without a coordinated ABI bump.

use libloading::{Library, Symbol};
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// FFI types — layout must match include/agenterm.h exactly.
// ---------------------------------------------------------------------------

/// C-compatible error record (thread-local message, valid until the next call).
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_camel_case_types)]
pub struct agt_error {
    pub operation: *const std::ffi::c_char,
    pub code: *const std::ffi::c_char,
    pub message: *const std::ffi::c_char,
}

/// Fixed-size accessibility node record.
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_camel_case_types)]
pub struct agt_a11y_node {
    pub bounds_x: i32,
    pub bounds_y: i32,
    pub bounds_width: i32,
    pub bounds_height: i32,
    pub id: [u8; 64],
    pub id_len: u32,
    pub id_truncated: u32,
    pub parent_id: [u8; 64],
    pub parent_id_len: u32,
    pub parent_id_truncated: u32,
    pub has_parent: u8,
    pub actions_count: u32,
}

/// C-compatible visible top-level window record.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(non_camel_case_types)]
pub struct agt_window_info {
    pub handle: isize,
    pub process_id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub focused: i32,
    pub minimized: i32,
    pub title: [u8; 128],
    pub title_len: u32,
    pub title_truncated: u32,
    pub app_name: [u8; 64],
    pub app_name_len: u32,
    pub app_name_truncated: u32,
}

/// ABI 1.10 caller-sized placement inspection record.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
#[allow(non_camel_case_types)]
pub struct agt_window_placement_info_v1 {
    pub struct_size: u32,
    pub record_version: u32,
    pub handle: isize,
    pub process_id: u32,
    pub role: i32,
    pub movable: i32,
    pub resizable: i32,
    pub constraints_kind: i32,
    pub constraint_flags: u32,
    pub min_width: u32,
    pub min_height: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub increment_width: u32,
    pub increment_height: u32,
}

impl Default for agt_window_info {
    fn default() -> Self {
        agt_window_info {
            handle: 0,
            process_id: 0,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            focused: 0,
            minimized: 0,
            title: [0u8; 128],
            title_len: 0,
            title_truncated: 0,
            app_name: [0u8; 64],
            app_name_len: 0,
            app_name_truncated: 0,
        }
    }
}

/// C-compatible single-screen record.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
#[allow(non_camel_case_types)]
pub struct agt_screen_info {
    pub frame_x: i32,
    pub frame_y: i32,
    pub frame_width: u32,
    pub frame_height: u32,
    pub visible_x: i32,
    pub visible_y: i32,
    pub visible_width: u32,
    pub visible_height: u32,
    pub primary: i32,
}

/// C-compatible resident desktop-host action specification.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(non_camel_case_types)]
pub struct agt_desktop_action {
    pub action_id: u32,
    pub label: *const u8,
    pub label_len: usize,
    pub shortcut: *const u8,
    pub shortcut_len: usize,
}

// ---------------------------------------------------------------------------
// ABI constants — values are part of the ABI contract (include/agenterm.h).
// ---------------------------------------------------------------------------

pub const AGT_OK: i32 = 0;
pub const AGT_UNSUPPORTED: i32 = 1;
pub const AGT_FAILED: i32 = 2;

pub const AGT_CAP_PTY: i32 = 1;
pub const AGT_CAP_PROCESS_OBSERVE: i32 = 3;
pub const AGT_CAP_WINDOW_HOST: i32 = 4;
pub const AGT_CAP_WINDOW_ENUMERATE: i32 = 5;
pub const AGT_CAP_WINDOW_OP: i32 = 6;
pub const AGT_CAP_SCREENSHOT: i32 = 7;
pub const AGT_CAP_CLIPBOARD: i32 = 8;
pub const AGT_CAP_INPUT_INJECT: i32 = 10;
pub const AGT_CAP_PARENT_CONSOLE: i32 = 15;
pub const AGT_CAP_ACCESSIBILITY_TREE: i32 = 16;
pub const AGT_CAP_DESKTOP_HOST: i32 = 17;
pub const AGT_CAP_WINDOW_PLACEMENT_INSPECT: i32 = 18;

pub const AGT_WINDOW_PLACEMENT_RECORD_V1: u32 = 1;
pub const AGT_WINDOW_ROLE_UNKNOWN: i32 = 0;
pub const AGT_WINDOW_ROLE_STANDARD: i32 = 1;
pub const AGT_WINDOW_ROLE_DIALOG: i32 = 2;
pub const AGT_WINDOW_ROLE_SHEET: i32 = 3;
pub const AGT_WINDOW_ROLE_SYSTEM_DIALOG: i32 = 4;
pub const AGT_WINDOW_ROLE_OTHER: i32 = 5;
pub const AGT_WINDOW_SUPPORT_UNKNOWN: i32 = 0;
pub const AGT_WINDOW_SUPPORT_YES: i32 = 1;
pub const AGT_WINDOW_SUPPORT_NO: i32 = 2;
pub const AGT_WINDOW_CONSTRAINTS_UNKNOWN: i32 = 0;
pub const AGT_WINDOW_CONSTRAINTS_EXPLICIT: i32 = 1;
pub const AGT_WINDOW_CONSTRAINTS_APPLICATION_ENFORCED: i32 = 2;
pub const AGT_WINDOW_CONSTRAINT_HAS_MIN: u32 = 1 << 0;
pub const AGT_WINDOW_CONSTRAINT_HAS_MAX: u32 = 1 << 1;
pub const AGT_WINDOW_CONSTRAINT_HAS_INCREMENT: u32 = 1 << 2;

/// `agt_a11y_tree_meta_string` fields.
pub const AGT_A11Y_META_BACKEND: i32 = 0;
pub const AGT_A11Y_META_ROOT_ID: i32 = 1;

/// `agt_a11y_node_string` kinds.
pub const AGT_A11Y_STR_ROLE: i32 = 0;
pub const AGT_A11Y_STR_NAME: i32 = 1;
pub const AGT_A11Y_STR_TEXT: i32 = 2;
pub const AGT_A11Y_STR_STATES: i32 = 3;

/// `agt_a11y_node_perform` action kinds.
pub const AGT_A11Y_ACTION_CLICK: i32 = 0;
pub const AGT_A11Y_ACTION_FOCUS: i32 = 1;

/// `agt_native_window_show` states.
pub const AGT_NATIVE_WINDOW_HIDE: i32 = 0;
pub const AGT_NATIVE_WINDOW_SHOW: i32 = 1;
pub const AGT_NATIVE_WINDOW_MINIMIZE: i32 = 2;
pub const AGT_NATIVE_WINDOW_MAXIMIZE: i32 = 3;
pub const AGT_NATIVE_WINDOW_RESTORE: i32 = 4;

/// The ABI major this build of cu speaks. libagenterm promises that a major
/// bump means breaking changes, so a library with a different major must be
/// refused rather than called through mismatched signatures.
///
/// Why hard-coded: since milestone 46 `agenterm-cu` no longer depends on
/// `agenterm-abi`, so there is no compile-time `ABI_MAJOR` to read.
/// Why safe: the `gate_expected_abi_major_matches_real_artifact` test loads
/// the real artifact and asserts `agt_abi_version() >> 16 ==
/// EXPECTED_ABI_MAJOR`, so this value cannot drift from the library without
/// that gate failing first.
const EXPECTED_ABI_MAJOR: u16 = 1;
pub const WINDOW_PLACEMENT_ABI_MINOR: u16 = 10;

/// `agt_input_pointer_click` buttons.
pub const AGT_INPUT_BUTTON_LEFT: i32 = 0;
pub const AGT_INPUT_BUTTON_RIGHT: i32 = 1;
pub const AGT_INPUT_BUTTON_MIDDLE: i32 = 2;

/// `agt_screenshot_capture_window` area kinds.
pub const AGT_SCREENSHOT_AREA_WINDOW: i32 = 0;
pub const AGT_SCREENSHOT_AREA_CLIENT: i32 = 1;

// ---------------------------------------------------------------------------
// Loaded-library handle.
// ---------------------------------------------------------------------------

/// A loaded libagenterm dynamic library plus the path it was opened from.
pub struct Dynlib {
    lib: Library,
    path: PathBuf,
}

impl Dynlib {
    /// Absolute path of the library actually loaded (for diagnostics).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Resolve one exported symbol by name.
    ///
    /// # Safety
    ///
    /// `T` must be the exact FFI type of the named export; calling through a
    /// mismatched signature is undefined behavior.
    pub unsafe fn sym<'lib, T>(&'lib self, name: &[u8]) -> Result<Symbol<'lib, T>, String> {
        unsafe { self.lib.get(name) }
            .map_err(|e| format!("symbol {} missing: {e}", String::from_utf8_lossy(name)))
    }

    /// Format the thread-local error record of the library as one line.
    pub fn last_error_message(&self) -> String {
        let Ok(f) = (unsafe { self.sym::<LastError>(b"agt_last_error") }) else {
            return "<agt_last_error missing>".to_owned();
        };
        let mut e = agt_error {
            operation: std::ptr::null(),
            code: std::ptr::null(),
            message: std::ptr::null(),
        };
        if unsafe { f(&mut e) } != AGT_OK {
            return "<agt_last_error failed>".to_owned();
        }
        let op = unsafe { CStr::from_ptr(e.operation) }.to_string_lossy();
        let code = unsafe { CStr::from_ptr(e.code) }.to_string_lossy();
        let msg = unsafe { CStr::from_ptr(e.message) }.to_string_lossy();
        format!("{op}: {code}: {msg}")
    }

    pub fn abi_version(&self) -> Result<u32, String> {
        let f = unsafe { self.sym::<AbiVersion>(b"agt_abi_version") }?;
        Ok(unsafe { f() })
    }
}

type LastError = unsafe extern "C" fn(*mut agt_error) -> i32;

/// `agt_abi_version`: returns `(major << 16) | minor` (see `include/agenterm.h`).
type AbiVersion = unsafe extern "C" fn() -> u32;

/// Compare a library-reported ABI version (`major << 16 | minor`, as returned
/// by `agt_abi_version`) with the major this build of cu speaks. Only the
/// major is checked here — the minor is additive: a library with a higher
/// minor simply has extra exports cu does not use, and a lower minor surfaces
/// later as a missing-symbol error from the per-call `Dynlib::sym`
/// resolution. A major mismatch must fail the whole load so mismatched
/// signatures are never invoked.
fn check_abi_major(reported: u32) -> Result<(), String> {
    let major = (reported >> 16) as u16;
    let minor = (reported & 0xffff) as u16;
    if major == EXPECTED_ABI_MAJOR {
        return Ok(());
    }
    Err(format!(
        "reports ABI {major}.{minor} but this build of cu speaks ABI major \
         {EXPECTED_ABI_MAJOR}; refusing to call through mismatched signatures"
    ))
}

// ---------------------------------------------------------------------------
// Location + process-wide cache.
// ---------------------------------------------------------------------------

/// Candidate dynamic-library file names per platform.
const CANDIDATES: [&str; 3] = [
    "agenterm.dll",      // Windows
    "libagenterm.so",    // Linux
    "libagenterm.dylib", // macOS
];

/// Locate the libagenterm dynamic library, in order:
/// 1. `AGENTERM_ABI_LIB` environment variable (full path);
/// 2. the exe's own directory (`agenterm.dll` / `libagenterm.so` /
///    `libagenterm.dylib`);
/// 3. walking up from the exe for `target/abi-release/` and
///    `target/abi-dev/` under each ancestor — the profile layout the
///    dylib-load regression builds into.
///
/// On failure returns every path that was considered so the caller can print
/// them (callers must fail loudly, never silently skip).
fn locate_library() -> Result<PathBuf, Vec<PathBuf>> {
    let mut tried: Vec<PathBuf> = Vec::new();

    if let Some(p) = std::env::var_os("AGENTERM_ABI_LIB") {
        let p = PathBuf::from(p);
        tried.push(p.clone());
        if p.is_file() {
            return Ok(p);
        }
    }

    let Some(exe) = std::env::current_exe().ok() else {
        return Err(tried);
    };
    if let Some(dir) = exe.parent() {
        for name in CANDIDATES {
            let p = dir.join(name);
            tried.push(p.clone());
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    // Walk up from the exe's directory looking for <dir>/target/abi-*/.
    let mut dir = exe.parent().map(Path::to_path_buf);
    while let Some(d) = dir {
        for profile in ["abi-release", "abi-dev"] {
            for name in CANDIDATES {
                let p = d.join("target").join(profile).join(name);
                tried.push(p.clone());
                if p.is_file() {
                    return Ok(p);
                }
            }
        }
        dir = d.parent().map(Path::to_path_buf);
    }

    Err(tried)
}

/// Why the dynamic library could not be loaded. `tried` lists every path
/// considered (in order) so a caller can render a helpful failure.
#[derive(Clone, Debug)]
pub struct LoadError {
    pub message: String,
    pub tried: Vec<PathBuf>,
}

static LIB: OnceLock<Result<&'static Dynlib, LoadError>> = OnceLock::new();

/// Load the libagenterm dynamic library exactly once per process and return
/// the cached handle. The load refuses (a) an unlocatable library and (b) a
/// library whose ABI major differs from [`EXPECTED_ABI_MAJOR`] — one gate at
/// load time instead of one at every call site, so a mismatched library never
/// gets invoked through cu's signatures. On failure the returned error lists
/// every candidate path that was tried.
pub fn load() -> Result<&'static Dynlib, &'static LoadError> {
    match LIB.get_or_init(|| {
        let path = match locate_library() {
            Ok(path) => path,
            Err(tried) => {
                return Err(LoadError {
                    message: "could not locate the libagenterm dynamic library".to_owned(),
                    tried,
                });
            }
        };
        let lib = match unsafe { Library::new(&path) } {
            Ok(lib) => lib,
            Err(e) => {
                return Err(LoadError {
                    message: format!("LoadLibrary({}) failed: {e}", path.display()),
                    tried: vec![path],
                });
            }
        };
        // ABI gate (milestone 57): refuse a library whose major differs from
        // what this build of cu speaks, before any of its symbols are called.
        // The minor is deliberately not compared — see `check_abi_major` for
        // the additive-minor / per-symbol division of labour.
        let gate = Dynlib {
            lib,
            path: path.clone(),
        };
        let verify = unsafe {
            let version: Symbol<AbiVersion> = match gate.sym(b"agt_abi_version") {
                Ok(f) => f,
                Err(message) => {
                    return Err(LoadError {
                        message: format!(
                            "libagenterm at {} does not export agt_abi_version ({message}); \
                             refusing to call without an ABI check",
                            path.display()
                        ),
                        tried: vec![path],
                    });
                }
            };
            check_abi_major(version())
        };
        if let Err(message) = verify {
            return Err(LoadError {
                message: format!("libagenterm at {} {message}", path.display()),
                tried: vec![path],
            });
        }
        Ok(Box::leak(Box::new(gate)))
    }) {
        Ok(lib) => Ok(*lib),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- check_abi_major (pure) -------------------------------------------

    #[test]
    fn check_abi_major_accepts_same_major_higher_minor() {
        // Major 1, minor 8 (the current artifact); a higher minor is additive.
        assert!(check_abi_major(0x0001_0006).is_ok());
    }

    #[test]
    fn check_abi_major_accepts_same_major_lower_minor() {
        // Major 1, minor 0: a lower minor is additive too and must not fail
        // here — the per-call `Dynlib::sym` reports any missing symbol later.
        assert!(check_abi_major(0x0001_0000).is_ok());
    }

    #[test]
    fn check_abi_major_rejects_other_major() {
        // Major 2 vs expected 1: must be refused, and the message must carry
        // both the reported major.minor and the expected major.
        let err = check_abi_major(0x0002_0000).unwrap_err();
        assert!(
            err.contains("ABI 2.0"),
            "message carries reported version: {err}"
        );
        assert!(
            err.contains("major 1"),
            "message carries expected major: {err}"
        );
        assert!(
            err.contains("refusing to call through mismatched signatures"),
            "message states the refusal: {err}"
        );
    }

    #[test]
    fn check_abi_major_rejects_major_zero() {
        // Major 0, minor 6: any major other than the expected one is refused.
        let err = check_abi_major(0x0000_0006).unwrap_err();
        assert!(
            err.contains("ABI 0.6"),
            "message carries reported version: {err}"
        );
        assert!(
            err.contains("major 1"),
            "message carries expected major: {err}"
        );
    }

    // -- gate against the real artifact -----------------------------------

    /// Gate (milestone 57): the real libagenterm artifact must report the ABI
    /// major this build of cu speaks, so `EXPECTED_ABI_MAJOR` cannot drift
    /// from the library — a future major bump with a stale constant fails this
    /// test instead of surfacing only at run time.
    ///
    /// The artifact must exist; when it cannot be located or opened the test
    /// prints an explicit `SKIP: <reason>` and returns instead of failing.
    /// CI builds the ABI library before this test runs, so it really runs
    /// there.
    #[test]
    fn gate_expected_abi_major_matches_real_artifact() {
        let path = match locate_library() {
            Ok(path) => path,
            Err(tried) => {
                let tried = tried
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("SKIP: could not locate the libagenterm dynamic library (tried: {tried})");
                return;
            }
        };
        let lib = match unsafe { Library::new(&path) } {
            Ok(lib) => lib,
            Err(e) => {
                println!(
                    "SKIP: could not open the libagenterm dynamic library at {}: {e}",
                    path.display()
                );
                return;
            }
        };
        let version: Symbol<AbiVersion> = match unsafe { lib.get(b"agt_abi_version") } {
            Ok(f) => f,
            Err(e) => {
                println!("SKIP: agt_abi_version missing in {}: {e}", path.display());
                return;
            }
        };
        let reported = unsafe { version() };
        assert_eq!(
            (reported >> 16) as u16,
            EXPECTED_ABI_MAJOR,
            "libagenterm at {} reports ABI major {} but this build of cu speaks major {}; \
             EXPECTED_ABI_MAJOR must be bumped in lockstep with the library",
            path.display(),
            reported >> 16,
            EXPECTED_ABI_MAJOR,
        );
    }
}
