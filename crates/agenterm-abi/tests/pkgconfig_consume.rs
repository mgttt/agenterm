//! Milestone 52 end-to-end gate: prove that a STATIC consumer using ONLY our
//! published pkg-config metadata really compiles, links and runs.
//!
//! `pkgconfig_libs.rs` compares source texts (README table <-> system_libs
//! constants <-> generate-pc.sh baked-in values) — a gate on the *source*
//! of the metadata. This test gates the *artifact*: it builds a staging
//! install tree under the system temp dir (never the repo tree), runs the
//! repository's real `generate-pc.sh` to produce `libagenterm.pc` (never a
//! second copy of the substitution logic — what is tested is what is
//! shipped), feeds `pkg-config --cflags --libs --static` output verbatim
//! (split on whitespace) to the real C toolchain, and runs the result. If
//! the generated `.pc` syntax is wrong, a placeholder survives, or
//! `Libs.private` order is broken, this test fails where the consumer
//! would fail.
//!
//! SKIP policy is STRICT by design (counted in CI logs): Windows -> SKIP
//! (`.pc` is a Unix delivery channel); no `pkg-config` executable -> SKIP.
//! Everything else MUST run — a missing C compiler is a hard failure, never
//! a skip, so a runner without a toolchain turns red instead of silently
//! skipping its way to green.
//!
//! C-toolchain discovery and artifact lookup are shared from
//! `common::toolchain` — the same "compiler found or SKIP" and
//! `AGENTERM_ABI_PROFILE_DIR` decisions as `c_static_link.rs`.

mod common;

use common::toolchain::{
    Cleanup, DIR_SEQ, find_c_compiler, locate_staticlib, repo_root, run_or_panic,
};
use std::process::Command;
use std::sync::atomic::Ordering;

/// The `version` value from `crates/agenterm-abi/Cargo.toml` — the
/// `@VERSION@` substitute. Read from the manifest so an ABI bump can never
/// leave the test handing the script a stale hard-coded version.
fn abi_crate_version() -> String {
    let manifest = repo_root().join("crates/agenterm-abi/Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest.display()));
    text.lines()
        .find_map(|line| {
            let rest = line.trim().strip_prefix("version = ")?;
            rest.trim()
                .strip_prefix('"')?
                .strip_suffix('"')
                .map(str::to_owned)
        })
        .unwrap_or_else(|| panic!("no `version = \"...\"` line in {}", manifest.display()))
}

#[test]
fn pkg_config_static_consumer_links_and_runs() {
    // Windows: .pc is a Unix delivery channel — explicit SKIP, never a run.
    if cfg!(windows) {
        eprintln!("SKIP: .pc files are a Unix delivery channel (this is Windows)");
        return;
    }

    // pkg-config must actually exist. Missing executable = explicit SKIP.
    let pkg_config_ok = Command::new("pkg-config")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if !pkg_config_ok {
        eprintln!("SKIP: no pkg-config executable on PATH");
        return;
    }

    // Everything past this point MUST run: a missing C compiler is a hard
    // failure, not a SKIP (strict SKIP policy, milestone 52).
    let Some(compiler) = find_c_compiler("pkgconfig_consume") else {
        panic!(
            "pkgconfig_consume: no C compiler found on PATH (strict SKIP policy: \
             a Unix pkg-config consumer needs a C toolchain — do not skip this gate)"
        );
    };

    let root = repo_root();
    let staticlib = locate_staticlib();
    let c_file = root.join("examples/c/agenterm_probe.c");
    assert!(c_file.is_file(), "missing {}", c_file.display());

    // ---- staging install tree (system temp dir, never the repo tree) -----
    let seq = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let stage =
        std::env::temp_dir().join(format!("agenterm-pkgconfig-{}-{seq}", std::process::id()));
    let lib_dir = stage.join("lib");
    let include_dir = stage.join("include");
    let pc_dir = lib_dir.join("pkgconfig");
    // Both leaves, explicitly: creating pc_dir brings lib_dir with it but says
    // nothing about include_dir, and the copy below is the only thing that
    // would notice — on a platform this test skips.
    std::fs::create_dir_all(&pc_dir).expect("create staging pkgconfig dir");
    std::fs::create_dir_all(&include_dir).expect("create staging include dir");
    let _cleanup = Cleanup(stage.clone());

    let lib_name = staticlib
        .file_name()
        .and_then(|s| s.to_str())
        .expect("staticlib has a file name");
    std::fs::copy(&staticlib, lib_dir.join(lib_name))
        .unwrap_or_else(|e| panic!("copy staticlib {} -> staging: {e}", staticlib.display()));
    std::fs::copy(
        root.join("include/agenterm.h"),
        include_dir.join("agenterm.h"),
    )
    .unwrap_or_else(|e| panic!("copy include/agenterm.h -> staging: {e}"));

    // ---- generate the .pc with the REPOSITORY script (never a re-implemen-
    // tation of its substitution logic in this test) ------------------------
    let pc = pc_dir.join("libagenterm.pc");
    let script = root.join("packaging/pkgconfig/generate-pc.sh");
    run_or_panic(
        "generate-pc.sh",
        Command::new("sh")
            .arg(&script)
            .arg("--prefix")
            .arg(&stage)
            .arg("--libdir")
            .arg(&lib_dir)
            .arg("--includedir")
            .arg(&include_dir)
            .arg("--version")
            .arg(abi_crate_version())
            .arg("--output")
            .arg(&pc),
    );

    // ---- pkg-config: cflags + static link line ----------------------------
    let pc_out = Command::new("pkg-config")
        .env("PKG_CONFIG_PATH", &pc_dir)
        .args(["--cflags", "--libs", "--static", "libagenterm"])
        .output()
        .expect("spawn pkg-config");
    assert!(
        pc_out.status.success(),
        "pkg-config --cflags --libs --static libagenterm failed\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&pc_out.stdout),
        String::from_utf8_lossy(&pc_out.stderr),
    );
    let link_line = String::from_utf8_lossy(&pc_out.stdout).trim().to_string();
    eprintln!(
        "pkgconfig_consume: pkg-config line: {link_line}\n\
         pkgconfig_consume: generated {}\n{}",
        pc.display(),
        std::fs::read_to_string(&pc).unwrap_or_default(),
    );
    assert!(
        !link_line.is_empty(),
        "pkg-config --cflags --libs --static libagenterm produced empty output"
    );
    assert!(
        link_line.contains("-lagenterm"),
        "pkg-config output must reference -lagenterm, got: {link_line:?}"
    );
    assert!(
        !link_line.contains('@'),
        "pkg-config output still carries a raw '@' (a @...@ placeholder was \
         not substituted in the generated .pc): {link_line:?}"
    );

    // ---- feed the link line VERBATIM (whitespace-split) to the C compiler.
    // probe.c comes first so every -l/-framework the line adds is placed
    // after the object that references the symbols (single-pass linkers). --
    let pc_args: Vec<&str> = link_line.split_whitespace().collect();
    let exe = stage.join("agenterm_probe");
    let mut cc = Command::new(&compiler.path);
    for (k, v) in &compiler.env {
        cc.env(k, v);
    }
    cc.arg("-Wall").arg("-Wextra").arg("-Werror");
    cc.arg(&c_file);
    cc.args(&pc_args);
    cc.arg("-o").arg(&exe);
    run_or_panic("pkg-config static compile/link", &mut cc);

    // ---- run once: exit code 0 is the static-consumer proof ---------------
    let out = run_or_panic("pkg-config static probe run", &mut Command::new(&exe));
    print!("{}", String::from_utf8_lossy(&out.stdout));
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
}
