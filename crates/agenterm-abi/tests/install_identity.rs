//! Milestone 56 gate: the shared library's INSTALL IDENTITY.
//!
//! Milestones 51–55 pinned the symbol surface (55 exports present and exact
//! in both dynamic and static artifacts, every archive symbol classified,
//! pkg-config end-to-end linkable), but nobody measured how the built
//! library identifies itself for INSTALLATION — the two fields that let a
//! library be shared by multiple programs and located at runtime:
//!
//! - ELF (`libagenterm.so`): `DT_SONAME` in the dynamic segment. Without it
//!   a consumer's `DT_NEEDED` records whatever filename the linker was given
//!   at build time, so versioned coexistence (`libagenterm.so.0` / `.so.1`)
//!   is impossible.
//! - Mach-O (`libagenterm.dylib`): the `LC_ID_DYLIB` install name. Cargo
//!   builds default to writing the BUILD-TIME ABSOLUTE PATH into it (e.g.
//!   `<build-tree>/target/abi-release/libagenterm.dylib`); a consumer then records that build-directory path and cannot find the
//!   library after it is installed elsewhere. This is the well-known Rust
//!   packaging trap.
//! - PE (`agenterm.dll`): Windows has neither concept. We measure and
//!   explicitly SKIP — never pretend a PE has a SONAME/install name.
//!
//! Gate semantics: first UNCONDITIONALLY print the measured values (so the
//! CI log always shows reality), then assert:
//! - ELF: `DT_SONAME` MUST exist and be non-empty.
//! - Mach-O: the install name MUST NOT leak the build tree (must start with
//!   `@rpath/`), with the measured value printed verbatim on failure.
//!
//! The first CI run on macOS/Linux is EXPECTED to be red — that is the
//! measurement; `crates/agenterm-abi/build.rs` (milestone 56) fixes the
//! fields. Do not weaken the assertions to make them green.

use common::toolchain::{locate_cdylib, profile_name};
use object::Endian;
use object::read::elf::{DynamicTable, FileHeader};
use object::read::macho::{LoadCommandIterator, LoadCommandVariant};
use object::read::{File, ReadRef};
use std::path::Path;

mod common;

/// Extract the ELF `DT_SONAME` string from the dynamic segment, or None when
/// absent. Pure parsing with `object` — never shelling out to readelf.
fn elf_soname<'data, Elf: FileHeader, R: ReadRef<'data>>(
    table: &DynamicTable<'data, Elf, R>,
) -> Option<String> {
    for d in table.iter() {
        if d.tag == object::elf::DT_SONAME {
            return table
                .string(d)
                .ok()
                .map(|raw| String::from_utf8_lossy(raw).into_owned());
        }
    }
    None
}

/// Extract the Mach-O `LC_ID_DYLIB` install name, or None when absent.
fn macho_install_name<'data, E: Endian>(
    endian: E,
    commands: LoadCommandIterator<'data, E>,
) -> Option<String> {
    for lc in commands {
        let Ok(lc) = lc else { continue };
        let Ok(variant) = lc.variant() else { continue };
        if let LoadCommandVariant::IdDylib(cmd) = variant {
            return lc
                .string(endian, cmd.dylib.name)
                .ok()
                .map(|raw| String::from_utf8_lossy(raw).into_owned());
        }
    }
    None
}

/// Print the measured ELF identity, then assert `DT_SONAME` exists and is
/// non-empty. The failure message carries the measured value verbatim.
fn report_and_assert_elf(lib: &Path, profile: &str, soname: Option<String>) {
    match &soname {
        Some(s) => eprintln!(
            "install_identity: kind=elf soname={s:?} (path={}, profile={})",
            lib.display(),
            profile
        ),
        None => eprintln!(
            "install_identity: kind=elf soname=<none> (path={}, profile={})",
            lib.display(),
            profile
        ),
    }
    let Some(s) = soname else {
        panic!(
            "install_identity: ELF DT_SONAME missing on {} (profile={}); measured soname=<none>. \
             Set it in crates/agenterm-abi/build.rs \
             (cargo:rustc-cdylib-link-arg=-Wl,-soname,libagenterm.so.<ABI major>)",
            lib.display(),
            profile
        );
    };
    assert!(
        !s.is_empty(),
        "install_identity: ELF DT_SONAME empty on {} (profile={}); measured soname=<empty>",
        lib.display(),
        profile
    );
}

/// Print the measured Mach-O identity, then assert the install name does not
/// leak the build tree and starts with `@rpath/`. Measured value verbatim in
/// every failure message.
fn report_and_assert_macho(lib: &Path, profile: &str, install_name: Option<String>) {
    match &install_name {
        Some(s) => eprintln!(
            "install_identity: kind=macho install_name={s:?} (path={}, profile={})",
            lib.display(),
            profile
        ),
        None => eprintln!(
            "install_identity: kind=macho install_name=<none> (path={}, profile={})",
            lib.display(),
            profile
        ),
    }
    let Some(s) = install_name else {
        panic!(
            "install_identity: Mach-O LC_ID_DYLIB missing on {} (profile={}); measured \
             install_name=<none>. Set it in crates/agenterm-abi/build.rs \
             (cargo:rustc-cdylib-link-arg=-Wl,-install_name,@rpath/libagenterm.dylib)",
            lib.display(),
            profile
        );
    };
    assert!(
        !(s.starts_with('/') && s.contains("/target/")),
        "install_identity: Mach-O install_name leaks the build tree on {} (profile={}); measured \
         install_name={s:?}. Set it to @rpath/libagenterm.dylib in crates/agenterm-abi/build.rs \
         (cargo:rustc-cdylib-link-arg=-Wl,-install_name,@rpath/libagenterm.dylib)",
        lib.display(),
        profile
    );
    assert!(
        s.starts_with("@rpath/"),
        "install_identity: Mach-O install_name should start with @rpath/ on {} (profile={}); \
         measured install_name={s:?}",
        lib.display(),
        profile
    );
}

#[test]
fn install_identity() {
    let lib = locate_cdylib();
    let profile = profile_name();
    let data = std::fs::read(&lib)
        .unwrap_or_else(|e| panic!("failed to read cdylib {}: {e}", lib.display()));
    let file = File::parse(&*data)
        .unwrap_or_else(|e| panic!("object::File::parse({}) failed: {e}", lib.display()));

    match &file {
        File::Elf32(f) => {
            let soname = f.elf_dynamic_table().ok().as_ref().and_then(elf_soname);
            report_and_assert_elf(&lib, &profile, soname);
        }
        File::Elf64(f) => {
            let soname = f.elf_dynamic_table().ok().as_ref().and_then(elf_soname);
            report_and_assert_elf(&lib, &profile, soname);
        }
        File::MachO32(f) => {
            let install_name = f
                .macho_load_commands()
                .ok()
                .and_then(|c| macho_install_name(f.endian(), c));
            report_and_assert_macho(&lib, &profile, install_name);
        }
        File::MachO64(f) => {
            let install_name = f
                .macho_load_commands()
                .ok()
                .and_then(|c| macho_install_name(f.endian(), c));
            report_and_assert_macho(&lib, &profile, install_name);
        }
        // Windows has no SONAME / LC_ID_DYLIB concept — explicitly SKIP
        // instead of pretending there is one.
        File::Pe32(_) | File::Pe64(_) => {
            eprintln!(
                "install_identity: SKIP: PE has no SONAME/LC_ID_DYLIB concept (path={}, profile={})",
                lib.display(),
                profile
            );
        }
        other => panic!(
            "install_identity: unexpected object format for {} (profile={}): {other:?} \
             (expected ELF / Mach-O / PE)",
            lib.display(),
            profile
        ),
    }
}
