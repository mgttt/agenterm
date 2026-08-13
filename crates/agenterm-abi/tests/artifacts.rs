//! Milestone 16 artifact gate: the three `[lib] crate-type` artifacts —
//! cdylib / staticlib / rlib — must all exist in the active profile
//! directory. The staticlib is the new delivery shape: if it is missing,
//! C/C++ consumers cannot statically link, so this test FAILS on purpose
//! instead of skipping (same policy as dylib_load.rs).
//!
//! Locating logic is copied from tests/dylib_load.rs — do not invent a new
//! path rule here.

use std::path::PathBuf;

/// The test binary lives in `target/<profile>/deps/`; the artifacts in
/// `target/<profile>/` (with rlib also present in `deps/`). Mirrors
/// `cdylib_path()` in tests/dylib_load.rs.
fn artifact_search_dirs() -> (PathBuf, PathBuf) {
    let exe = std::env::current_exe().expect("current_exe()");
    let deps = exe.parent().expect("test binary has a parent dir");
    let profile_dir = deps.parent().expect("deps dir has a parent dir");
    (profile_dir.to_path_buf(), deps.to_path_buf())
}

/// Per-platform artifact file names, keyed by crate-type. The staticlib and
/// the cdylib's import library can both be named `agenterm.lib` on
/// Windows (the import library is `agenterm.dll.lib`, so no clash).
struct ArtifactNames {
    cdylib: &'static [&'static str],
    staticlib: &'static [&'static str],
    rlib: &'static str,
}

fn artifact_names() -> ArtifactNames {
    #[cfg(windows)]
    {
        ArtifactNames {
            cdylib: &["agenterm.dll"],
            staticlib: &["agenterm.lib"],
            rlib: "libagenterm.rlib",
        }
    }
    #[cfg(target_os = "linux")]
    {
        ArtifactNames {
            cdylib: &["libagenterm.so"],
            staticlib: &["libagenterm.a"],
            rlib: "libagenterm.rlib",
        }
    }
    #[cfg(target_os = "macos")]
    {
        ArtifactNames {
            cdylib: &["libagenterm.dylib"],
            staticlib: &["libagenterm.a"],
            rlib: "libagenterm.rlib",
        }
    }
}

/// Find `name` in either the profile dir or the deps dir. Returns the full
/// path of the first hit.
fn find(dir_a: &PathBuf, dir_b: &PathBuf, name: &str) -> Option<PathBuf> {
    for dir in [dir_a, dir_b] {
        let p = dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

#[test]
fn all_three_crate_type_artifacts_exist() {
    let (profile_dir, deps_dir) = artifact_search_dirs();
    let names = artifact_names();

    let mut missing: Vec<String> = Vec::new();

    let cdylib_hit = names
        .cdylib
        .iter()
        .find_map(|n| find(&profile_dir, &deps_dir, n));
    if cdylib_hit.is_none() {
        missing.push(format!("cdylib (candidates: {:?})", names.cdylib));
    }

    let staticlib_hit = names
        .staticlib
        .iter()
        .find_map(|n| find(&profile_dir, &deps_dir, n));
    if staticlib_hit.is_none() {
        missing.push(format!("staticlib (candidates: {:?})", names.staticlib));
    }

    let rlib_hit = find(&profile_dir, &deps_dir, names.rlib);
    if rlib_hit.is_none() {
        missing.push(format!("rlib (candidate: {:?})", names.rlib));
    }

    assert!(
        missing.is_empty(),
        "agenterm-abi artifacts missing. Searched {} and {}. Missing: {} \
         — build with an unwind profile first, e.g. \
         `cargo build -p agenterm-abi --profile abi-release`",
        profile_dir.display(),
        deps_dir.display(),
        missing.join("; ")
    );

    // Sanity: the staticlib must actually be a library, not an empty stub.
    let st = staticlib_hit.expect("staticlib hit checked above");
    let len = std::fs::metadata(&st)
        .unwrap_or_else(|e| panic!("stat {st:?}: {e}"))
        .len();
    assert!(len > 0, "staticlib {st:?} exists but is zero bytes");

    let dl = cdylib_hit.expect("cdylib hit checked above");
    let len = std::fs::metadata(&dl)
        .unwrap_or_else(|e| panic!("stat {dl:?}: {e}"))
        .len();
    assert!(len > 0, "cdylib {dl:?} exists but is zero bytes");

    let rl = rlib_hit.expect("rlib hit checked above");
    let len = std::fs::metadata(&rl)
        .unwrap_or_else(|e| panic!("stat {rl:?}: {e}"))
        .len();
    assert!(len > 0, "rlib {rl:?} exists but is zero bytes");
}
