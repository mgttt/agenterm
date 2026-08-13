//! agenterm-cu: milestone 44/46 — runtime dynamic-library demo.
//!
//! This binary REALLY loads the libagenterm dynamic library at run time
//! (`dlopen` on Unix, `LoadLibrary` on Windows) via the shared
//! `agenterm_cu::dynlib` layer (which caches the load process-wide), instead
//! of linking `agenterm-abi` as a static rlib. Every symbol below is resolved
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

use libloading::Symbol;
use std::ffi::{CStr, c_char};

use agenterm_cu::dynlib::{self, agt_window_info};

// ---------------------------------------------------------------------------
// FFI types — layout must match include/agenterm.h exactly.
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // ABI-layout struct: every field must exist even if unread.
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

// ---------------------------------------------------------------------------
// Capability ids and statuses (values are part of the ABI contract and must
// match include/agenterm.h; the constants live in `dynlib`).
// ---------------------------------------------------------------------------

/// Capabilities the demo probes, in display order.
const CAPABILITIES: [(&str, i32); 10] = [
    ("PTY", dynlib::AGT_CAP_PTY),
    ("WINDOW_HOST", dynlib::AGT_CAP_WINDOW_HOST),
    ("SCREENSHOT", dynlib::AGT_CAP_SCREENSHOT),
    ("PROCESS_OBSERVE", dynlib::AGT_CAP_PROCESS_OBSERVE),
    ("CLIPBOARD", dynlib::AGT_CAP_CLIPBOARD),
    ("PARENT_CONSOLE", dynlib::AGT_CAP_PARENT_CONSOLE),
    ("WINDOW_ENUMERATE", dynlib::AGT_CAP_WINDOW_ENUMERATE),
    ("WINDOW_OP", dynlib::AGT_CAP_WINDOW_OP),
    ("INPUT_INJECT", dynlib::AGT_CAP_INPUT_INJECT),
    ("ACCESSIBILITY_TREE", dynlib::AGT_CAP_ACCESSIBILITY_TREE),
];

// ---------------------------------------------------------------------------
// Export signatures — identical to crates/agenterm-abi/tests/dylib_load.rs.
// ---------------------------------------------------------------------------

type AbiVersion = unsafe extern "C" fn() -> u32;
type BuildId = unsafe extern "C" fn() -> *const c_char;
type CapabilityQuery = unsafe extern "C" fn(i32) -> i32;
type WindowEnumerate = unsafe extern "C" fn(*mut agt_window_info, usize, *mut usize) -> i32;
type ProcessList = unsafe extern "C" fn(*mut agt_process_info, usize, *mut usize) -> i32;
type ProcessSelf = unsafe extern "C" fn() -> u32;
type RuntimeDefaultShell = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;

// ---------------------------------------------------------------------------
// Two-stage read-only probes (cap=0 probe, allocate, fetch).
// ---------------------------------------------------------------------------

/// `agt_window_enumerate`: probe with cap=0, allocate the required count,
/// fetch. Returns the records written. Titles are never copied out — only
/// `title_len` is reported.
unsafe fn window_enumerate(lib: &dynlib::Dynlib) -> Result<Vec<agt_window_info>, String> {
    let f: Symbol<WindowEnumerate> = unsafe { lib.sym(b"agt_window_enumerate") }?;
    let mut needed = 0usize;
    let st = unsafe { f(std::ptr::null_mut(), 0, &mut needed) };
    if st == dynlib::AGT_UNSUPPORTED {
        return Err(
            "agt_window_enumerate: AGT_UNSUPPORTED — window enumeration not available on this host"
                .to_owned(),
        );
    }
    if st != dynlib::AGT_FAILED {
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
    if st != dynlib::AGT_OK {
        return Err(format!(
            "agt_window_enumerate fetch: {}",
            lib.last_error_message()
        ));
    }
    buf.truncate(got);
    Ok(buf)
}

/// `agt_process_list`: two-stage, returns the number of live processes.
unsafe fn process_list_count(lib: &dynlib::Dynlib) -> Result<usize, String> {
    let f: Symbol<ProcessList> = unsafe { lib.sym(b"agt_process_list") }?;
    let mut needed = 0usize;
    let st = unsafe { f(std::ptr::null_mut(), 0, &mut needed) };
    if st != dynlib::AGT_FAILED {
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
    if st != dynlib::AGT_OK {
        return Err(format!(
            "agt_process_list fetch: {}",
            lib.last_error_message()
        ));
    }
    Ok(got)
}

/// `agt_runtime_default_shell`: two-stage, returns only the path LENGTH —
/// the path itself is never printed (privacy).
unsafe fn default_shell_length(lib: &dynlib::Dynlib) -> Result<usize, String> {
    let f: Symbol<RuntimeDefaultShell> = unsafe { lib.sym(b"agt_runtime_default_shell") }?;
    let mut needed = 0usize;
    let st = unsafe { f(std::ptr::null_mut(), 0, &mut needed) };
    if st != dynlib::AGT_FAILED {
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
    if st != dynlib::AGT_OK {
        return Err(format!(
            "agt_runtime_default_shell fetch: {}",
            lib.last_error_message()
        ));
    }
    Ok(got)
}

// ---------------------------------------------------------------------------
// Demo orchestration.
// ---------------------------------------------------------------------------

/// Everything the demo collected.
struct Demo {
    library_path: String,
    abi_version: u32,
    build_id: String,
    capabilities: Vec<(&'static str, i32)>,
    windows: Vec<agt_window_info>,
    self_pid: u32,
    process_count: usize,
    default_shell_len: usize,
}

/// Exercise the read-only exports of the loaded library.
unsafe fn run(lib: &dynlib::Dynlib) -> Result<Demo, String> {
    let abi_version: Symbol<AbiVersion> = unsafe { lib.sym(b"agt_abi_version") }?;
    let build_id: Symbol<BuildId> = unsafe { lib.sym(b"agt_build_id") }?;
    let capability: Symbol<CapabilityQuery> = unsafe { lib.sym(b"agt_capability_query") }?;
    let process_self: Symbol<ProcessSelf> = unsafe { lib.sym(b"agt_process_self") }?;

    let abi_version = unsafe { abi_version() };
    let build_id = unsafe { CStr::from_ptr(build_id()) }
        .to_string_lossy()
        .into_owned();

    let mut capabilities = Vec::with_capacity(CAPABILITIES.len());
    for (name, cap) in CAPABILITIES {
        let st = unsafe { capability(cap) };
        if st != dynlib::AGT_OK && st != dynlib::AGT_UNSUPPORTED {
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
        library_path: lib.path().display().to_string(),
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
    if st == dynlib::AGT_OK {
        "OK"
    } else {
        "UNSUPPORTED"
    }
}

fn print_human(d: &Demo) {
    println!("agenterm-cu — runtime libagenterm dlopen demo (milestone 44)");
    println!("loaded library : {}", d.library_path);
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
        "library_path": d.library_path,
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

fn fail(msg: &str, tried: Option<&[std::path::PathBuf]>, json: bool) -> ! {
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

    let lib = match dynlib::load() {
        Ok(lib) => lib,
        Err(error) => {
            fail(&error.message, Some(&error.tried), json);
        }
    };

    match unsafe { run(lib) } {
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
