//! agenterm-cu: milestone 44 — runtime dynamic-library demo.
//!
//! This binary REALLY loads the libagenterm dynamic library at run time
//! (`dlopen` on Unix, `LoadLibrary` on Windows) via `libloading`, instead of
//! linking `agenterm-abi` as a static rlib. Every symbol below is resolved
//! from the loaded `.dll` / `.so` / `.dylib` and called through the FFI.
//!
//! Strictly read-only: it never calls any export that changes system state —
//! no `agt_input_*`, no `agt_native_window_*`, no screenshot writes, no
//! clipboard access. This is the user's desktop.
//!
//! Human output shows the absolute path of the library actually loaded, the
//! abi version, build id, capability statuses, a top-level window
//! enumeration (count + first 5 records; window titles are printed as
//! LENGTHS only, never content), a process probe (self pid + live process
//! count) and the default-shell path length. `--json` emits the same data as
//! one JSON object for downstream tooling.

use libloading::{Library, Symbol};
use std::ffi::{CStr, c_char};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// FFI types — layout must match include/agenterm.h exactly.
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // ABI-layout struct: every field must exist even if unread.
#[allow(non_camel_case_types)]
struct agt_error {
    operation: *const c_char,
    code: *const c_char,
    message: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)]
#[allow(non_camel_case_types)]
struct agt_process_info {
    id: u32,
    parent_id: u32,
    name: [u8; 64],
    name_len: u32,
    name_truncated: u32,
}

impl Default for agt_process_info {
    fn default() -> Self {
        agt_process_info {
            id: 0,
            parent_id: 0,
            name: [0u8; 64],
            name_len: 0,
            name_truncated: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)]
#[allow(non_camel_case_types)]
struct agt_window_info {
    handle: isize,
    process_id: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    focused: i32,
    minimized: i32,
    title: [u8; 128],
    title_len: u32,
    title_truncated: u32,
    app_name: [u8; 64],
    app_name_len: u32,
    app_name_truncated: u32,
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

// ---------------------------------------------------------------------------
// Status codes (agt_status) and capability ids (agt_capability) — values are
// part of the ABI contract and must match include/agenterm.h.
// ---------------------------------------------------------------------------

const AGT_OK: i32 = 0;
const AGT_UNSUPPORTED: i32 = 1;
const AGT_FAILED: i32 = 2;

const AGT_CAP_PTY: i32 = 1;
const AGT_CAP_PROCESS_OBSERVE: i32 = 3;
const AGT_CAP_WINDOW_HOST: i32 = 4;
const AGT_CAP_WINDOW_ENUMERATE: i32 = 5;
const AGT_CAP_WINDOW_OP: i32 = 6;
const AGT_CAP_SCREENSHOT: i32 = 7;
const AGT_CAP_CLIPBOARD: i32 = 8;
const AGT_CAP_INPUT_INJECT: i32 = 10;
const AGT_CAP_PARENT_CONSOLE: i32 = 15;
const AGT_CAP_ACCESSIBILITY_TREE: i32 = 16;

/// Capabilities the demo probes, in display order.
const CAPABILITIES: [(&str, i32); 10] = [
    ("PTY", AGT_CAP_PTY),
    ("WINDOW_HOST", AGT_CAP_WINDOW_HOST),
    ("SCREENSHOT", AGT_CAP_SCREENSHOT),
    ("PROCESS_OBSERVE", AGT_CAP_PROCESS_OBSERVE),
    ("CLIPBOARD", AGT_CAP_CLIPBOARD),
    ("PARENT_CONSOLE", AGT_CAP_PARENT_CONSOLE),
    ("WINDOW_ENUMERATE", AGT_CAP_WINDOW_ENUMERATE),
    ("WINDOW_OP", AGT_CAP_WINDOW_OP),
    ("INPUT_INJECT", AGT_CAP_INPUT_INJECT),
    ("ACCESSIBILITY_TREE", AGT_CAP_ACCESSIBILITY_TREE),
];

// ---------------------------------------------------------------------------
// Export signatures — identical to crates/agenterm-abi/tests/dylib_load.rs.
// ---------------------------------------------------------------------------

type AbiVersion = unsafe extern "C" fn() -> u32;
type BuildId = unsafe extern "C" fn() -> *const c_char;
type CapabilityQuery = unsafe extern "C" fn(i32) -> i32;
type LastError = unsafe extern "C" fn(*mut agt_error) -> i32;
type WindowEnumerate = unsafe extern "C" fn(*mut agt_window_info, usize, *mut usize) -> i32;
type ProcessList = unsafe extern "C" fn(*mut agt_process_info, usize, *mut usize) -> i32;
type ProcessSelf = unsafe extern "C" fn() -> u32;
type RuntimeDefaultShell = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;

// ---------------------------------------------------------------------------
// Dynamic-library location (candidate names from tests/dylib_load.rs).
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
/// them (the demo must fail loudly, never silently skip).
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

// ---------------------------------------------------------------------------
// Symbol plumbing.
// ---------------------------------------------------------------------------

/// Resolve one exported symbol by name.
unsafe fn sym<'l, T>(lib: &'l Library, name: &[u8]) -> Result<Symbol<'l, T>, String> {
    unsafe { lib.get(name) }
        .map_err(|e| format!("symbol {} missing: {e}", String::from_utf8_lossy(name)))
}

/// Format the thread-local error record of the library as one line.
fn last_error_message(lib: &Library) -> String {
    let Ok(f) = (unsafe { lib.get::<LastError>(b"agt_last_error") }) else {
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

// ---------------------------------------------------------------------------
// Two-stage read-only probes (cap=0 probe, allocate, fetch).
// ---------------------------------------------------------------------------

/// `agt_window_enumerate`: probe with cap=0, allocate the required count,
/// fetch. Returns the records written. Titles are never copied out — only
/// `title_len` is reported.
unsafe fn window_enumerate(lib: &Library) -> Result<Vec<agt_window_info>, String> {
    let f: Symbol<WindowEnumerate> = unsafe { sym(lib, b"agt_window_enumerate") }?;
    let mut needed = 0usize;
    let st = unsafe { f(std::ptr::null_mut(), 0, &mut needed) };
    if st == AGT_UNSUPPORTED {
        return Err(
            "agt_window_enumerate: AGT_UNSUPPORTED — window enumeration not available on this host"
                .to_owned(),
        );
    }
    if st != AGT_FAILED {
        return Err(format!(
            "agt_window_enumerate probe: expected AGT_FAILED (buffer_too_small), got {st}"
        ));
    }
    if needed == 0 {
        return Ok(Vec::new());
    }
    let mut buf = vec![agt_window_info::default(); needed];
    let mut got = 0usize;
    let st = unsafe { f(buf.as_mut_ptr(), needed, &mut got) };
    if st != AGT_OK {
        return Err(format!(
            "agt_window_enumerate fetch: {}",
            last_error_message(lib)
        ));
    }
    buf.truncate(got);
    Ok(buf)
}

/// `agt_process_list`: two-stage, returns the number of live processes.
unsafe fn process_list_count(lib: &Library) -> Result<usize, String> {
    let f: Symbol<ProcessList> = unsafe { sym(lib, b"agt_process_list") }?;
    let mut needed = 0usize;
    let st = unsafe { f(std::ptr::null_mut(), 0, &mut needed) };
    if st != AGT_FAILED {
        return Err(format!(
            "agt_process_list probe: expected AGT_FAILED (buffer_too_small), got {st}"
        ));
    }
    if needed == 0 {
        return Ok(0);
    }
    let mut buf = vec![agt_process_info::default(); needed];
    let mut got = 0usize;
    let st = unsafe { f(buf.as_mut_ptr(), needed, &mut got) };
    if st != AGT_OK {
        return Err(format!(
            "agt_process_list fetch: {}",
            last_error_message(lib)
        ));
    }
    Ok(got)
}

/// `agt_runtime_default_shell`: two-stage, returns only the path LENGTH —
/// the path itself is never printed (privacy).
unsafe fn default_shell_length(lib: &Library) -> Result<usize, String> {
    let f: Symbol<RuntimeDefaultShell> = unsafe { sym(lib, b"agt_runtime_default_shell") }?;
    let mut needed = 0usize;
    let st = unsafe { f(std::ptr::null_mut(), 0, &mut needed) };
    if st != AGT_FAILED {
        return Err(format!(
            "agt_runtime_default_shell probe: expected AGT_FAILED (buffer_too_small), got {st}"
        ));
    }
    if needed == 0 {
        return Ok(0);
    }
    let mut buf = vec![0u8; needed];
    let mut got = 0usize;
    let st = unsafe { f(buf.as_mut_ptr(), needed, &mut got) };
    if st != AGT_OK {
        return Err(format!(
            "agt_runtime_default_shell fetch: {}",
            last_error_message(lib)
        ));
    }
    Ok(got)
}

// ---------------------------------------------------------------------------
// Demo orchestration.
// ---------------------------------------------------------------------------

/// Everything the demo collected.
struct Demo {
    library_path: PathBuf,
    abi_version: u32,
    build_id: String,
    capabilities: Vec<(&'static str, i32)>,
    windows: Vec<agt_window_info>,
    self_pid: u32,
    process_count: usize,
    default_shell_len: usize,
}

/// Exercise the read-only exports of the loaded library.
unsafe fn run(lib: &Library, library_path: &Path) -> Result<Demo, String> {
    let abi_version: Symbol<AbiVersion> = unsafe { sym(lib, b"agt_abi_version") }?;
    let build_id: Symbol<BuildId> = unsafe { sym(lib, b"agt_build_id") }?;
    let capability: Symbol<CapabilityQuery> = unsafe { sym(lib, b"agt_capability_query") }?;
    let process_self: Symbol<ProcessSelf> = unsafe { sym(lib, b"agt_process_self") }?;

    let abi_version = unsafe { abi_version() };
    let build_id = unsafe { CStr::from_ptr(build_id()) }
        .to_string_lossy()
        .into_owned();

    let mut capabilities = Vec::with_capacity(CAPABILITIES.len());
    for (name, cap) in CAPABILITIES {
        let st = unsafe { capability(cap) };
        if st != AGT_OK && st != AGT_UNSUPPORTED {
            return Err(format!(
                "agt_capability_query({name}) returned unexpected status {st}"
            ));
        }
        capabilities.push((name, st));
    }

    let windows = unsafe { window_enumerate(lib) }?;
    let self_pid = unsafe { process_self() };
    let process_count = unsafe { process_list_count(lib) }?;
    let default_shell_len = unsafe { default_shell_length(lib) }?;

    Ok(Demo {
        library_path: library_path.to_path_buf(),
        abi_version,
        build_id,
        capabilities,
        windows,
        self_pid,
        process_count,
        default_shell_len,
    })
}

// ---------------------------------------------------------------------------
// Output.
// ---------------------------------------------------------------------------

fn status_label(st: i32) -> &'static str {
    if st == AGT_OK { "OK" } else { "UNSUPPORTED" }
}

fn print_human(d: &Demo) {
    println!("agenterm-cu — runtime libagenterm dlopen demo (milestone 44)");
    println!("loaded library : {}", d.library_path.display());
    let major = (d.abi_version >> 16) & 0xffff;
    let minor = d.abi_version & 0xffff;
    println!("abi_version    : {major}.{minor} (0x{:08x})", d.abi_version);
    println!("build_id       : {}", d.build_id);
    println!("capabilities:");
    for (name, st) in &d.capabilities {
        println!("  {:<18} {}", name, status_label(*st));
    }
    println!("windows (visible top-level):");
    println!("  count: {}", d.windows.len());
    for (i, w) in d.windows.iter().take(5).enumerate() {
        println!(
            "  [{}] handle=0x{:x} pid={} {}x{} focused={} title_len={}",
            i + 1,
            w.handle as u64,
            w.process_id,
            w.width,
            w.height,
            w.focused,
            w.title_len
        );
    }
    if d.windows.len() > 5 {
        println!("  ... ({} more)", d.windows.len() - 5);
    }
    println!("processes:");
    println!("  self_pid: {}", d.self_pid);
    println!("  count   : {}", d.process_count);
    println!("default_shell:");
    println!("  length  : {} bytes", d.default_shell_len);
}

fn print_json(d: &Demo) {
    let mut capabilities = serde_json::Map::new();
    for (name, st) in &d.capabilities {
        capabilities.insert((*name).to_owned(), status_label(*st).into());
    }
    let windows: Vec<serde_json::Value> = d
        .windows
        .iter()
        .map(|w| {
            serde_json::json!({
                "handle": format!("0x{:x}", w.handle as u64),
                "pid": w.process_id,
                "x": w.x,
                "y": w.y,
                "width": w.width,
                "height": w.height,
                "focused": w.focused,
                "title_len": w.title_len,
            })
        })
        .collect();
    let out = serde_json::json!({
        "ok": true,
        "library_path": d.library_path.to_string_lossy(),
        "abi": {
            "major": (d.abi_version >> 16) & 0xffff,
            "minor": d.abi_version & 0xffff,
            "raw": d.abi_version,
            "build_id": d.build_id,
        },
        "capabilities": capabilities,
        "windows": { "count": d.windows.len(), "sample": windows },
        "processes": { "self_pid": d.self_pid, "count": d.process_count },
        "default_shell_len": d.default_shell_len,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_owned())
    );
}

fn fail(msg: &str, tried: Option<&[PathBuf]>, json: bool) -> ! {
    if json {
        let value = match tried {
            Some(paths) => serde_json::json!({
                "ok": false,
                "error": msg,
                "tried": paths.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>(),
            }),
            None => serde_json::json!({ "ok": false, "error": msg }),
        };
        println!("{value}");
    }
    eprintln!("agenterm-cu: {msg}");
    if let Some(paths) = tried {
        eprintln!("Tried, in order:");
        for p in paths {
            eprintln!("  {}", p.display());
        }
        eprintln!("Build it first: cargo build -p agenterm-abi --profile abi-release");
        eprintln!(
            "or point AGENTERM_ABI_LIB at an existing agenterm.dll / libagenterm.so / libagenterm.dylib"
        );
    }
    std::process::exit(1);
}

fn main() {
    let json = std::env::args().skip(1).any(|a| a == "--json");

    let library_path = match locate_library() {
        Ok(p) => p,
        Err(tried) => {
            fail(
                "could not locate the libagenterm dynamic library",
                Some(&tried),
                json,
            );
        }
    };

    let lib = match unsafe { Library::new(&library_path) } {
        Ok(lib) => Box::leak(Box::new(lib)),
        Err(e) => {
            let msg = format!("LoadLibrary({}) failed: {e}", library_path.display());
            fail(&msg, None, json);
        }
    };

    match unsafe { run(lib, &library_path) } {
        Ok(demo) => {
            if json {
                print_json(&demo);
            } else {
                print_human(&demo);
            }
        }
        Err(msg) => fail(&msg, None, json),
    }
}
