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
    /// Bare libm names (`sqrt`, `ceil`, `fabs`, `fmod`, `fmax`, `roundeven`,
    /// the `f16`/`f128` variants, ...) that compiler-builtins defines so
    /// float support works without libm. These are the one class here whose
    /// names a consumer's own C code really can produce — they ARE the libm
    /// interface — so membership additionally REQUIRES a WEAK binding: the
    /// consumer's (or the platform libm's) strong definition then wins at
    /// link time and ours is discarded. A STRONG definition of any of these
    /// would silently rebind a consumer's `sqrt` to ours, so it stays
    /// unclassified and turns this gate red. Measured on Linux CI, where all
    /// of them are weak.
    LibmWeak,
}

/// The bare libm names compiler-builtins may define. Measured from the Linux
/// CI archive (milestone 55, first real run); extend from measured data.
/// Base names only — the `f`/`f16`/`f128` suffixes are handled by the
/// matcher, and `fmaximum_num`-style names carry their suffix after `_num`.
fn is_libm_name(name: &str) -> bool {
    const LIBM_BASES: &[&str] = &[
        "cbrt",
        "ceil",
        "copysign",
        "fabs",
        "fdim",
        "floor",
        "fma",
        "fmax",
        "fmaximum",
        "fmaximum_num",
        "fmin",
        "fminimum",
        "fminimum_num",
        "fmod",
        "rint",
        "round",
        "roundeven",
        "sqrt",
        "trunc",
    ];
    // libm spells the precision as a suffix: none = f64, `f` = f32,
    // `f16`/`f128` = the new float types. Longest match first is unnecessary
    // because we compare the whole name against base+suffix.
    const SUFFIXES: &[&str] = &["", "f", "f16", "f128"];
    LIBM_BASES.iter().any(|base| {
        SUFFIXES.iter().any(|suffix| {
            name.len() == base.len() + suffix.len() && name == format!("{base}{suffix}")
        })
    })
}

/// Classify a symbol name, or `None` when it belongs to no known class.
/// Platform-specific rules are split with `#[cfg]`; each platform's
/// "unclassified" set must still be EMPTY (that is the assertion below).
fn classify(name: &str, is_weak: bool) -> Option<Class> {
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
    // ELF emits a `DW.ref.<personality>` data symbol per object that carries
    // an LSDA, so the unwinder can find the personality routine.
    if let Some(referent) = n.strip_prefix("DW.ref.")
        && (referent == "rust_eh_personality" || referent.starts_with("_Unwind_"))
    {
        return Some(Class::Unwind);
    }
    if is_platform_toolchain(n) {
        return Some(Class::Platform);
    }
    // Deliberately last, and deliberately conditional on the binding: a weak
    // libm definition loses to the consumer's, a strong one would hijack it.
    if is_weak && is_libm_name(n) {
        return Some(Class::LibmWeak);
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
        // Soft-float comparison helpers, one per predicate and width:
        // __eqdf2 __nesf2 __lttf2 __gehf2 __unordtf2 ... (measured on Linux).
        "__eq",
        "__ne",
        "__lt",
        "__le",
        "__gt",
        "__ge",
        "__unord",
        // Width conversions: __extendhfsf2, __truncdfhf2, __trunctfdf2, ...
        "__extend",
        "__trunc",
        // Integer powers: __powidf2 / __powisf2 / __powitf2.
        "__powi",
        // libgcc-compatible half<->float conversion aliases.
        "__gnu_f2h_ieee",
        "__gnu_h2f_ieee",
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
        // Non-MSVC Unix toolchains. `anon.*` is handled above. rustc plants
        // this one section symbol so a debugger can auto-load the pretty
        // printers; the `__rustc_` prefix is not a name C source produces.
        name == "__rustc_debug_gdb_scripts_section__"
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
/// Object-member symbols carry their binding: `LibmWeak` membership depends
/// on it. A name seen both weak and strong is reported as STRONG — the
/// dangerous reading is the one that must not be classified away.
fn archive_defined_symbols(lib_path: &Path) -> (Vec<(String, bool)>, Vec<String>) {
    let raw = std::fs::read(lib_path)
        .unwrap_or_else(|e| panic!("failed to read staticlib {}: {e}", lib_path.display()));
    let data: &[u8] = &raw;
    let archive = ArchiveFile::parse(data)
        .unwrap_or_else(|e| panic!("ArchiveFile::parse({}) failed: {e}", lib_path.display()));
    let mut object_names: std::collections::BTreeMap<String, bool> =
        std::collections::BTreeMap::new();
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
                for symbol in file.symbols() {
                    if symbol.is_global()
                        && symbol.is_definition()
                        && let Ok(name) = symbol.name()
                    {
                        if is_import_member {
                            import_names.push(name.to_string());
                        } else {
                            // A later strong definition overrides an earlier
                            // weak reading of the same name; never the other
                            // way round.
                            let weak = symbol.is_weak();
                            object_names
                                .entry(name.to_string())
                                .and_modify(|w| *w &= weak)
                                .or_insert(weak);
                        }
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
    import_names.sort();
    import_names.dedup();
    (object_names.into_iter().collect(), import_names)
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
    for (name, is_weak) in object_symbols {
        match classify(&name, is_weak) {
            Some(class) => *class_counts.entry(class).or_insert(0) += 1,
            // Binding is part of the report: a bare libm name shows up here
            // only when it is STRONG, and that distinction is the finding.
            None => unclassified.push(if is_weak {
                format!("{name} (weak)")
            } else {
                format!("{name} (STRONG)")
            }),
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
        "archive_surface: agt={} rust={} builtins={} unwind={} platform={} import={} libm_weak={} unclassified={:?} total={} ({}, profile={})",
        count(Class::Agt),
        count(Class::RustMangled),
        count(Class::Builtins),
        count(Class::Unwind),
        count(Class::Platform),
        count(Class::ImportLibrary),
        count(Class::LibmWeak),
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
