//! What the shipped executable demands of Windows *before* it runs.
//!
//! Every static import is resolved by the PE loader ahead of `main`. A single
//! import that the target system does not export therefore does not degrade
//! one feature — it refuses the whole program, with a dialog naming a symbol
//! the user cannot act on. That failure mode is invisible to every other gate
//! in this repository, because they all run on a machine new enough to
//! satisfy the import.
//!
//! It shipped exactly that way: `CreatePseudoConsole`, `ResizePseudoConsole`
//! and `ClosePseudoConsole` were imported statically. They arrived in Windows
//! 10 build 17763 (1809), so on Windows Server 2016 (build 14393, still in
//! support) `agenterm-con.exe` could not start at all. The adapter now
//! resolves them through `GetProcAddress`, and this gate is what keeps them
//! resolved that way: re-adding the plain `use` is a one-line change that
//! nothing else would notice.
//!
//! Parsing uses the pure-Rust `object` crate rather than `dumpbin`, matching
//! `agenterm-abi/tests/export_exactness.rs`: one code path, no toolchain
//! binary on PATH, and a readable failure instead of parsed console text.

#![cfg(windows)]

use object::Object;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Imports that make the executable refuse to start on a supported Windows.
///
/// Kept as data rather than a single assertion so the failure message can say
/// *which* symbol and *which* Windows it locks the product out of. Add an
/// entry whenever a newer API becomes reachable statically.
/// Its weakness is that it is hand-written: it can only refuse what someone
/// already knew to list. Both entries below were added *after* a user's
/// machine found them, in two separate rounds. `tools/probe-imports.ps1`
/// exists so the next round is a full answer from the target system instead
/// of another guess from this one.
const LOAD_TIME_BLOCKERS: &[(&str, &str)] = &[
    ("CreatePseudoConsole", "Windows 10 build 17763 (1809)"),
    ("ResizePseudoConsole", "Windows 10 build 17763 (1809)"),
    ("ClosePseudoConsole", "Windows 10 build 17763 (1809)"),
    // Documented as 1607 — which *is* Server 2016 — and still absent there:
    // 1607 implements it only in KernelBase.dll and the kernel32 forwarder
    // arrived in 1703. Documented minimum versions are therefore evidence,
    // not proof; only the target system settles it.
    (
        "SetThreadDescription",
        "the kernel32 forwarder, Windows 10 build 15063 (1703)",
    ),
];

/// Modules the operating system itself provides on the oldest Windows this
/// product claims to run on, Windows Server 2016 (build 14393).
///
/// The `api-ms-win-*` entries are UCRT and Win32 API sets, which are OS
/// components from Windows 10 RTM onward — not a redistributable.
const OS_PROVIDED_MODULES: &[&str] = &[
    "advapi32.dll",
    "api-ms-win-core-synch-l1-2-0.dll",
    // The Universal CRT. Deliberately still dynamic: it ships with Windows,
    // so linking it statically would cost bytes to duplicate something the
    // target already has.
    "api-ms-win-crt-heap-l1-1-0.dll",
    "api-ms-win-crt-runtime-l1-1-0.dll",
    "api-ms-win-crt-string-l1-1-0.dll",
    "gdi32.dll",
    "gdiplus.dll",
    "imm32.dll",
    "kernel32.dll",
    "ntdll.dll",
    "shell32.dll",
    "user32.dll",
];

/// Modules that are *not* OS components and must be installed or shipped
/// beside the executable.
///
/// Empty, and that is the point: `VCRUNTIME140.dll` used to be here. It is a
/// redistributable, absent from a clean Windows Server 2016 until someone
/// installs the Visual C++ runtime, and it is now linked statically instead
/// (see `build.rs` and `startup.rs::__vcrt_initialize`). An empty list is
/// what makes a future redistributable dependency turn this gate red rather
/// than quietly widening what a user has to install.
const KNOWN_NON_OS_MODULES: &[&str] = &[];

fn shipped_binary() -> PathBuf {
    // An integration test runs from `target/<profile>/deps/`, so the binary
    // it exercises is two directories up — the same resolution the black-box
    // suite uses, and for the same reason: `CARGO_BIN_EXE_*` does not cover
    // bins declared in another package.
    let mut path = std::env::current_exe().expect("test executable path");
    path.pop();
    path.pop();
    path.push("agenterm-con.exe");
    assert!(
        path.is_file(),
        "agenterm-con is missing at {}; build it with \
         `cargo build -p agenterm-con --bin agenterm-con`",
        path.display()
    );
    path
}

/// Import names and their modules, lowercased for the module comparison
/// because the PE import table spells `KERNEL32.dll` and `kernel32.dll`
/// inconsistently within a single image.
fn imports() -> (BTreeSet<String>, BTreeSet<String>) {
    let path = shipped_binary();
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let file = object::File::parse(&*bytes)
        .unwrap_or_else(|error| panic!("cannot parse {} as a PE: {error}", path.display()));
    // Proves the parse found a real image rather than silently yielding an
    // empty table, which would make every assertion below vacuously pass.
    assert!(
        file.sections().next().is_some(),
        "parsed image has no sections"
    );

    let entries = file.imports().expect("PE import table");
    assert!(
        !entries.is_empty(),
        "no imports parsed; an empty table would make this gate assert nothing"
    );
    let symbols = entries
        .iter()
        .map(|import| String::from_utf8_lossy(import.name()).into_owned())
        .collect();
    let modules = entries
        .iter()
        .map(|import| String::from_utf8_lossy(import.library()).to_lowercase())
        .collect();
    (symbols, modules)
}

#[test]
fn no_static_import_locks_the_product_out_of_a_supported_windows() {
    let (symbols, _) = imports();
    let blocking: Vec<String> = LOAD_TIME_BLOCKERS
        .iter()
        .filter(|(symbol, _)| symbols.contains(*symbol))
        .map(|(symbol, since)| format!("{symbol} (needs {since})"))
        .collect();
    assert!(
        blocking.is_empty(),
        "agenterm-con.exe statically imports {blocking:?}. The PE loader \
         resolves these before `main`, so the program will not start on an \
         older supported Windows — it is not a degraded feature, it is an \
         entry-point dialog. Resolve them with GetProcAddress instead, as \
         `adapters/windows/pty.rs::conpty` does."
    );
}

#[test]
fn every_module_the_loader_needs_is_an_os_component_or_a_recorded_exception() {
    let (_, modules) = imports();
    let unexpected: Vec<&String> = modules
        .iter()
        .filter(|module| {
            !OS_PROVIDED_MODULES.contains(&module.as_str())
                && !KNOWN_NON_OS_MODULES.contains(&module.as_str())
        })
        .collect();
    assert!(
        unexpected.is_empty(),
        "agenterm-con.exe now depends on {unexpected:?}, which is neither a \
         Windows Server 2016 OS component nor a recorded exception. A module \
         the target system lacks stops the program at load time. Either drop \
         the dependency or add it to KNOWN_NON_OS_MODULES with what it costs \
         to deploy."
    );
    // The exception list is a debt record, not a wish list: an entry that no
    // longer applies must be deleted so it cannot excuse a future dependency.
    for known in KNOWN_NON_OS_MODULES {
        assert!(
            modules.contains(*known),
            "{known} is recorded as a known non-OS dependency but is no \
             longer imported; delete the entry"
        );
    }
}
