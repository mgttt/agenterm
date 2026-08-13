//! Size probe — variant B (dynamic).
//!
//! Milestone 15: the same six probes as variant A, but the mechanism code is
//! NOT statically linked. This binary depends only on `libloading` (plus
//! std); every capability comes from the `libagenterm` cdylib's C exports,
//! loaded at run time. If the mechanism code were statically linked too,
//! variant B's artifact would be much larger than it is.

use libloading::{Library, Symbol};
use std::path::PathBuf;
use std::process::ExitCode;

const AGT_OK: i32 = 0;
const AGT_UNSUPPORTED: i32 = 1;
const AGT_FAILED: i32 = 2;

/// C-compatible process record (mirror of `agt_process_info` in
/// include/agenterm.h). Only the layout matters; the fields are never read
/// by this probe.
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)]
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

type AbiVersion = unsafe extern "C" fn() -> u32;
/// Both two-stage UTF-8 exports (`agt_runtime_user_config_dir` and
/// `agt_runtime_default_shell`) share this exact FFI signature.
type RuntimeTwoStageUtf8 = unsafe extern "C" fn(*mut u8, usize, *mut usize) -> i32;
type ClipboardHasText = unsafe extern "C" fn() -> i32;
type ProcessList = unsafe extern "C" fn(*mut agt_process_info, usize, *mut usize) -> i32;
type ParentConsoleWriteStdout = unsafe extern "C" fn(*const u8, usize) -> i32;

const CDYLIB_NAMES: [&str; 3] = [
    "agenterm.dll",      // Windows
    "libagenterm.so",    // Linux
    "libagenterm.dylib", // macOS
];

/// Candidate locations relative to each ancestor of the probe executable.
/// `abi-release` / `abi-dev` are the libagenterm unwind profiles; `release`
/// / `debug` cover a dylib staged into the same profile directory.
const REL_CANDIDATES: [&str; 5] = ["", "abi-release/", "abi-dev/", "release/", "debug/"];

fn main() -> ExitCode {
    println!("size-probe variant B (dynamic: libagenterm cdylib via libloading)");
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("size-probe variant B: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let path = find_cdylib().ok_or(
        "libagenterm cdylib not found. Build it first, e.g. \
         `cargo build -p agenterm-abi --profile abi-release`, or set \
         AGENTERM_ABI_LIB to its path",
    )?;
    let lib = unsafe { Library::new(&path) }
        .map_err(|e| format!("LoadLibrary/dlopen({path:?}) failed: {e}"))?;

    // 1. user config dir length (two-stage; never print its content)
    let config_dir_len = two_stage_utf8_len(&lib, b"agt_runtime_user_config_dir")?;

    // 2. default shell length (two-stage; never print its content)
    let shell_len = two_stage_utf8_len(&lib, b"agt_runtime_default_shell")?;

    // 3. clipboard: does it hold Unicode text? (bool only)
    let has_text: Symbol<ClipboardHasText> = sym(&lib, b"agt_clipboard_has_text")?;
    let clipboard_has_text = unsafe { has_text() } != 0;

    // 4. process list entry count (two-stage; no big speculative allocation)
    let process_count = process_count(&lib)?;

    // 5. write one short line to the parent console
    let write: Symbol<ParentConsoleWriteStdout> = sym(&lib, b"agt_parent_console_write_stdout")?;
    let line = b"size-probe[variant B] parent-console write ok";
    let parent_console = match unsafe { write(line.as_ptr(), line.len()) } {
        AGT_OK => "ok",
        AGT_UNSUPPORTED => "unsupported",
        _ => "failed",
    };

    // 6. ABI version from the dylib export
    let abi_version: Symbol<AbiVersion> = sym(&lib, b"agt_abi_version")?;
    let abi_version = unsafe { abi_version() };

    println!("user_config_dir_len={config_dir_len}");
    println!("default_shell_len={shell_len}");
    println!("clipboard_has_text={clipboard_has_text}");
    println!("process_count={process_count}");
    println!("parent_console_write_stdout={parent_console}");
    println!("abi_version={abi_version}");
    Ok(())
}

/// Fetch a symbol by name.
fn sym<'l, T>(lib: &'l Library, name: &[u8]) -> Result<Symbol<'l, T>, String> {
    unsafe { lib.get(name) }
        .map_err(|e| format!("symbol {} missing: {e}", String::from_utf8_lossy(name)))
}

/// Two-stage UTF-8 probe (spec 3.4): call with `cap == 0` to learn the
/// required length, then allocate exactly that and call again. Returns the
/// number of bytes written. Never returns the content itself.
fn two_stage_utf8_len(lib: &Library, name: &[u8]) -> Result<usize, String> {
    let f: Symbol<RuntimeTwoStageUtf8> = sym(lib, name)?;
    let mut required = 0usize;
    // cap == 0 is the canonical "how big?" probe; buffer_too_small is expected.
    let s = unsafe { f(std::ptr::null_mut(), 0, &mut required) };
    if s != AGT_OK && s != AGT_FAILED {
        return Err(format!("{name:?} probe: unexpected status {s}"));
    }
    if s == AGT_OK {
        return Ok(required); // empty result
    }
    let mut buf = vec![0u8; required];
    let mut written = 0usize;
    let s = unsafe { f(buf.as_mut_ptr(), buf.len(), &mut written) };
    if s != AGT_OK {
        return Err(format!("{name:?} read: status {s}"));
    }
    Ok(written)
}

/// Two-stage process-list probe: `cap == 0` learns the required record
/// count, then the caller allocates exactly that many records and calls
/// again. Returns the written record count.
fn process_count(lib: &Library) -> Result<usize, String> {
    let f: Symbol<ProcessList> = sym(lib, b"agt_process_list")?;
    let mut required = 0usize;
    let s = unsafe { f(std::ptr::null_mut(), 0, &mut required) };
    if s != AGT_OK && s != AGT_FAILED {
        return Err(format!("agt_process_list probe: unexpected status {s}"));
    }
    if s == AGT_OK {
        return Ok(required); // empty result
    }
    let mut buf = vec![agt_process_info::default(); required];
    let mut count = 0usize;
    let s = unsafe { f(buf.as_mut_ptr(), buf.len(), &mut count) };
    if s != AGT_OK {
        return Err(format!("agt_process_list read: status {s}"));
    }
    Ok(count)
}

/// Locate the libagenterm cdylib: `AGENTERM_ABI_LIB` wins, then walk up
/// from the probe executable looking in candidate profile directories under
/// each ancestor. This mirrors how the dylib acceptance test finds the
/// cdylib, but also covers profiles in different directories.
fn find_cdylib() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("AGENTERM_ABI_LIB") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let mut dir: PathBuf = exe.parent()?.to_path_buf();
    loop {
        for rel in REL_CANDIDATES {
            let base = if rel.is_empty() {
                dir.clone()
            } else {
                dir.join(rel)
            };
            for name in CDYLIB_NAMES {
                let candidate = base.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        if !dir.pop() {
            break;
        }
    }
    None
}
