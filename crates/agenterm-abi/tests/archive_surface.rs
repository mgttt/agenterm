//! Milestone 55 gate: EVERY global DEFINED symbol in the static archive
//! (`agenterm.lib` on Windows / `libagenterm.a` on Unix) must belong to a
//! known class — and the set of unclassified symbols must be EMPTY.
//!
//! Why this is a real problem: every global symbol of a `.a` participates in
//! the CONSUMER's link. An UNMANGLED C symbol can collide with a symbol the
//! consumer itself defines; a Rust-mangled symbol carries the crate
//! disambiguator hash, so a collision is effectively impossible. The danger
//! is bare names, and this gate proves there are no bare names outside the
//! known, justified classes below.
//!
//! Milestone 54 measured the archive symbol surface on Windows only
//! (`agenterm.lib`: 5533 global DEFINED symbols — 4467 Rust-mangled, ~285
//! compiler-builtins weak symbols, 254 `__real@`/`__xmm@` constants, 175
//! `__imp_*`, 163 other `_`-prefixed, 55 `agt_*`, 9 `anon.*`). The Linux
//! figure (~372 non-`agt_` non-`_R` symbols) was INFERRED, not measured. This
//! gate turns the surface into a MEASURED gate on all three platforms: the
//! first CI run may legitimately report new unclassified names, and the
//! classes below get extended from that real data — never by adding a
//! catch-all "anything else is fine" bucket, which would defeat the gate.
//!
//! Parsing uses the pure-Rust `object` crate — one code path across the
//! COFF (Windows `.lib`) / ELF / Mach-O member formats, no shelling out to
//! lib.exe/nm/ar.

use common::toolchain::{locate_staticlib, profile_name};
use object::read::archive::ArchiveFile;
use object::read::coff::ImportFile;
use object::{File, Object, ObjectSymbol};
use std::path::Path;

mod common;

/// The known symbol classes. Each variant documents WHY the names it accepts
/// are safe for a consumer to link against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    /// `agt_*` — the promised C ABI surface (`exports.txt`). Safe by
    /// contract: it IS the interface, and consumers take it from the header.
    Agt,
    /// Rust v0 (`_R...`) / legacy (`_ZN...`) mangled names. Safe: they carry
    /// the crate disambiguator hash, so a name collision with consumer
    /// symbols is effectively impossible. (On Mach-O these names are stored
    /// raw — `_R...` already starts with `_`, so ld64 does not add a C
    /// prefix; legacy `_ZN...` likewise.)
    RustMangled,
    /// compiler-builtins (`__divdi3`, `__udivmodti4`, `__rust_i128_*`,
    /// `__addtf3`, ...; on Windows also as `.weak.__divdi3` /
    /// `__divdi3.default` weak/strong pairs). Safe: these are the
    /// well-known compiler-rt runtime helpers every Rust consumer already
    /// links, and a consumer cannot collide with them without also colliding
    /// with its own compiler runtime.
    Builtins,
    /// Unwind/personality routines (`rust_eh_personality`, `_Unwind_*`).
    /// Safe: they are the standard C++/Rust unwinder entry points provided
    /// by the platform toolchain; colliding with them would mean colliding
    /// with the runtime itself.
    Unwind,
    /// Platform/toolchain artifacts: MSVC `__real@`/`__xmm@` floating
    /// constants, `__imp_*` import thunks, `__IMPORT_DESCRIPTOR_*`,
    /// `??_C@` string literals, `$cppxdata$` EH data, `__chkstk`,
    /// `_fltused`, `__security_*`; LLVM `anon.*`. Safe: these live in the
    /// linker's private name space (`@`, `$`, `??`, `__imp_`), which C
    /// consumer source cannot produce; the `.weak.`/`.default` builtins
    /// variants are handled under `Builtins`.
    Platform,
    /// Symbols of Windows import-library members merged into the archive by
    /// the MSVC linker (`kernel32.dll`, `user32.dll`, `icu.dll`, ... members:
    /// `__imp_<api>` / `<api>` thunks, `<dll>_NULL_THUNK_DATA`). This is
    /// standard for EVERY MSVC static library, not agenterm's own surface:
    /// the thunks mirror the system DLL export table, the MSVC linker
    /// de-duplicates identical import members at consumer link time, and a
    /// consumer's own definition of the same name wins over the thunk. They
    /// are counted separately from the object-member surface below so the
    /// real agenterm symbol set is never diluted by toolchain noise.
    ImportLibrary,
}

/// Classify a symbol name, or `None` when it belongs to no known class.
/// Platform-specific rules are split with `#[cfg]`; each platform's
/// "unclassified" set must still be EMPTY (that is the assertion below).
fn classify(name: &str) -> Option<Class> {
    // Rust-mangled names match on their raw form on every platform (see the
    // `Class::RustMangled` doc). Mach-O prefixes plain C symbols with `_`,
    // so `agt_*` and the other C-space names are matched after stripping
    // exactly one leading `_` on macOS only.
    if name.starts_with("_R") || name.starts_with("_ZN") {
        return Some(Class::RustMangled);
    }
    let n = if cfg!(target_os = "macos") {
        name.strip_prefix('_').unwrap_or(name)
    } else {
        name
    };

    if n.starts_with("agt_") {
        return Some(Class::Agt);
    }
    if is_compiler_builtin(n) {
        return Some(Class::Builtins);
    }
    if n == "rust_eh_personality" || n.starts_with("_Unwind_") {
        return Some(Class::Unwind);
    }
    if is_platform_toolchain(n) {
        return Some(Class::Platform);
    }
    None
}

/// compiler-builtins runtime helpers. Matched by name shape; the set is
/// MEASURED (milestone 54 on Windows) and extended from real CI data when a
/// new builtin shows up — never by a catch-all.
fn is_compiler_builtin(name: &str) -> bool {
    // Windows weak/strong pairs emitted for builtins:
    //   `.weak.__divdi3` (weak implementation) and `__divdi3.default`
    //   (strong default). No consumer produces `.weak.`-prefixed or
    //   `.default`-suffixed C symbols.
    if name.starts_with(".weak.") || name.ends_with(".default") {
        return true;
    }
    // Known compiler-rt / compiler-builtins shapes.
    const BUILTIN_PREFIXES: &[&str] = &[
        "__rust_",       // __rust_i128_*, __rust_probestack, ...
        "__compilerrt_", // __compilerrt_abort_impl (compiler-rt's abort)
        "__div",
        "__mod",
        "__udiv",
        "__umod",
        "__mul",
        "__add",
        "__sub",
        "__fix",
        "__float",
        "__floatsi",
        "__floatt",
        "__clz",
        "__ctz",
        "__ffs",
        "__popcount",
        "__parity",
        "__bswap",
        "__ashl",
        "__ashr",
        "__lshr",
        "__neg",
        "__abs",
        "__cmp",
        "__ucmp",
        "__aeabi_", // ARM runtime ABI helpers
    ];
    BUILTIN_PREFIXES.iter().any(|p| name.starts_with(p))
}

/// Platform / toolchain artifacts. MSVC-heavy on Windows, LLVM `anon.*` on
/// the LLVM-based targets. Each entry names the real observed shape; extend
/// from measured CI data when a new one appears.
fn is_platform_toolchain(name: &str) -> bool {
    // LLVM anonymous symbols appear on every LLVM-based target (Windows
    // included): `anon.<hash>.<n>.llvm.<seed>`. They are global section
    // symbols LLVM emits for anonymous data/functions; no consumer source
    // can produce this shape.
    if name.starts_with("anon.") {
        return true;
    }
    #[cfg(target_env = "msvc")]
    {
        // MSVC toolchain artifacts (measured milestone 54 on Windows):
        if name.starts_with("__real@") // x87/SSE float constants
            || name.starts_with("__xmm@") // XMM constants
            || name.starts_with("__imp_") // import thunks
            || name.starts_with("__IMPORT_DESCRIPTOR_")
            || name.starts_with("??_") // MSVC string literals / vftables / RTTI
            || name.starts_with('$') // $cppxdata$, $filt$, ... EH data
            || name == "__chkstk"
            || name == "_fltused"
            || name.starts_with("__security_")
            || name.starts_with("__C_specific_handler")
            || name.starts_with("__GSHandlerCheck")
            || name.starts_with("_ftol2")
        {
            return true;
        }
        false
    }
    #[cfg(not(target_env = "msvc"))]
    {
        // Non-MSVC Unix toolchains: LLVM anonymous symbols (`anon.*`) are
        // handled above; nothing else observed yet. Extended from measured
        // CI data as the first runs report more.
        false
    }
}

/// All global DEFINED symbol names of the static archive, deduplicated and
/// split by member origin:
/// - `object` — symbols from real object members (agenterm's own surface);
/// - `import` — symbols from Windows import-library members (`<dll>.dll`),
///   MSVC toolchain noise that every MSVC static library carries.
///
/// `object::read::archive::ArchiveFile::parse` consumes the leading metadata
/// members (symbol table / linker member / names table), so the `members()`
/// iterator yields exactly the object and import members.
fn archive_defined_symbols(lib_path: &Path) -> (Vec<String>, Vec<String>) {
    let raw = std::fs::read(lib_path)
        .unwrap_or_else(|e| panic!("failed to read staticlib {}: {e}", lib_path.display()));
    let data: &[u8] = &raw;
    let archive = ArchiveFile::parse(data)
        .unwrap_or_else(|e| panic!("ArchiveFile::parse({}) failed: {e}", lib_path.display()));
    let mut object_names: Vec<String> = Vec::new();
    let mut import_names: Vec<String> = Vec::new();
    let mut member_count = 0usize;
    let mut unparsed_members: Vec<String> = Vec::new();
    for member in archive.members() {
        let member = member.unwrap_or_else(|e| panic!("archive member read failed: {e}"));
        member_count += 1;
        let member_data = member
            .data(data)
            .unwrap_or_else(|e| panic!("archive member data read failed: {e}"));
        // Windows import-library members are named after the DLL they import
        // from (`kernel32.dll`, `api-ms-win-*.dll`, ...). On Unix archives
        // every member is a real object file.
        let is_import_member = member.name().ends_with(b".dll");
        match File::parse(member_data) {
            Ok(file) => {
                let target = if is_import_member {
                    &mut import_names
                } else {
                    &mut object_names
                };
                for symbol in file.symbols() {
                    if symbol.is_global()
                        && symbol.is_definition()
                        && let Ok(name) = symbol.name()
                    {
                        target.push(name.to_string());
                    }
                }
            }
            Err(_) => {
                // Short-form COFF import members (`FileKind::CoffImport`) are
                // not object files: `File::parse` cannot read them, but
                // `ImportFile::parse` yields the imported symbol name.
                // Short imports only appear as `<dll>.dll` members on
                // Windows; if one ever shows up elsewhere it is a coverage
                // gap and must fail loudly.
                if is_import_member {
                    match ImportFile::parse(member_data) {
                        Ok(import) => {
                            import_names
                                .push(String::from_utf8_lossy(import.symbol()).into_owned());
                        }
                        Err(_) => {
                            unparsed_members
                                .push(String::from_utf8_lossy(member.name()).into_owned());
                        }
                    }
                } else {
                    unparsed_members.push(String::from_utf8_lossy(member.name()).into_owned());
                }
            }
        }
    }
    if !unparsed_members.is_empty() {
        panic!(
            "archive_surface: {} member(s) of {} were neither object files nor \
             import-library members (coverage gap, do not ignore): {:?}",
            unparsed_members.len(),
            lib_path.display(),
            unparsed_members
        );
    }
    if member_count == 0 {
        panic!(
            "archive_surface: {} has no members — is it the right artifact?",
            lib_path.display()
        );
    }
    object_names.sort();
    object_names.dedup();
    import_names.sort();
    import_names.dedup();
    (object_names, import_names)
}

/// The gate itself: classify every global defined symbol of the object
/// members (the real agenterm surface); the unclassified set must be empty.
/// Import-library member symbols are toolchain noise and count separately.
#[test]
fn archive_symbol_surface_is_classified() {
    let lib_path = locate_staticlib();
    let (object_symbols, import_symbols) = archive_defined_symbols(&lib_path);

    let mut class_counts: std::collections::BTreeMap<Class, usize> =
        std::collections::BTreeMap::new();
    let mut unclassified: Vec<String> = Vec::new();
    for name in object_symbols {
        match classify(&name) {
            Some(class) => *class_counts.entry(class).or_insert(0) += 1,
            None => unclassified.push(name),
        }
    }
    // Import-library member symbols go through the gate too: they belong to
    // the `ImportLibrary` class by member origin (never a catch-all — the
    // class is defined by WHERE the symbol came from and why that is safe).
    *class_counts.entry(Class::ImportLibrary).or_insert(0) += import_symbols.len();
    unclassified.sort();
    unclassified.dedup();

    let count = |class: Class| class_counts.get(&class).copied().unwrap_or(0);
    eprintln!(
        "archive_surface: agt={} rust={} builtins={} unwind={} platform={} import={} unclassified={:?} total={} ({}, profile={})",
        count(Class::Agt),
        count(Class::RustMangled),
        count(Class::Builtins),
        count(Class::Unwind),
        count(Class::Platform),
        count(Class::ImportLibrary),
        unclassified,
        class_counts.values().sum::<usize>() + unclassified.len(),
        lib_path.display(),
        profile_name()
    );

    assert!(
        unclassified.is_empty(),
        "archive_surface: {} global DEFINED symbol(s) from the OBJECT members \
         of the static archive {} belong to NO known class. Every symbol must \
         be justified — extend `classify()` with the real name and its safety \
         reason, never add a catch-all bucket:\n  {}",
        unclassified.len(),
        lib_path.display(),
        unclassified.join("\n  ")
    );
}
