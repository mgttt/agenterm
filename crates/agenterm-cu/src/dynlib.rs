//! Runtime dynamic-library loading shared by `cu`, `agenterm-cu` and the
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
}

type LastError = unsafe extern "C" fn(*mut agt_error) -> i32;

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
/// the cached handle. On failure the returned error lists every candidate
/// path that was tried.
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
        match unsafe { Library::new(&path) } {
            Ok(lib) => Ok(Box::leak(Box::new(Dynlib { lib, path }))),
            Err(e) => Err(LoadError {
                message: format!("LoadLibrary({}) failed: {e}", path.display()),
                tried: vec![path],
            }),
        }
    }) {
        Ok(lib) => Ok(*lib),
        Err(error) => Err(error),
    }
}
