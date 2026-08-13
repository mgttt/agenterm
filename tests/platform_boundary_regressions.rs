use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN_HOST_BRANCH_MARKERS: [(&str, &str); 12] = [
    ("is_windows_host(", "host predicate usage"),
    ("is_unix_host(", "host predicate usage"),
    ("platform_kind(", "platform kind dispatch"),
    ("cfg!(windows)", "compile-time windows branch"),
    ("cfg!(unix)", "compile-time unix branch"),
    ("#[cfg(windows)]", "compile-time windows branch"),
    ("#[cfg(unix)]", "compile-time unix branch"),
    ("cfg(target_os", "compile-time OS branch"),
    ("cfg(target_family", "compile-time OS branch"),
    ("cfg(target_arch", "compile-time architecture branch"),
    (
        "cfg(feature = \"windows\")",
        "compile-time windows feature branch",
    ),
    (
        "cfg(feature = \"unix\")",
        "compile-time unix feature branch",
    ),
];

// These four thin subsystem launchers are the target-selection boundary by
// design. The stronger native-boundary audit still rejects raw OS APIs in the
// three Rust-runtime entrypoints and separately constrains the size-bounded,
// no_std CUI trampoline.
//
// `crates/agenterm-con/src/startup.rs` joins them as a loader entry: it defines the
// linker-visible entry symbol and the `.CRT$X*` initializer arrays of its own
// binary, plus the per-architecture `global_asm!` thunk that jumps to rustc's
// generated `main`. All three are properties of the crate being linked, so
// there is no form of them that could live behind the platform crate's API.
// Keep this list in step with NATIVE_ENTRYPOINT_EXEMPTIONS in
// src/platform/boundary_tests.rs -- the two suites audit the same tree and a
// file exempt from one but not the other reddens only the lane that runs both.
const HOST_BRANCH_ENTRYPOINT_EXEMPTIONS: &[&str] = &[
    "src/bin/agenterm.rs",
    "src/bin/agenterm-cc.rs",
    "crates/agenterm-con/src/main.rs",
    "src/bin/agenterm-com.rs",
    "crates/agenterm-con/src/startup.rs",
];

#[test]
fn non_platform_src_has_no_runtime_host_branching_references() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs_files(&src_root, &mut files).expect("collect src tree");
    collect_rs_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/agenterm-con/src"),
        &mut files,
    )
    .expect("collect agenterm-con src tree");

    let mut violations = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .expect("source is below repository root")
            .to_string_lossy()
            .replace('\\', "/");
        if HOST_BRANCH_ENTRYPOINT_EXEMPTIONS.contains(&relative.as_str()) {
            continue;
        }
        let content = fs::read_to_string(&file).expect("read source file");
        for (line_index, line) in content.lines().enumerate() {
            let mut violations_in_line = Vec::new();
            for (needle, reason) in &FORBIDDEN_HOST_BRANCH_MARKERS {
                if line.contains(needle) {
                    violations_in_line.push(format!(
                        "line {}: {} ({})",
                        line_index + 1,
                        line.trim(),
                        reason
                    ));
                }
            }
            if line.contains("PlatformKind::") {
                violations_in_line.push(format!(
                    "line {}: {} ({})",
                    line_index + 1,
                    line.trim(),
                    "explicit platform enum usage"
                ));
            }
            if !violations_in_line.is_empty() {
                violations.push(format!(
                    "{}:\n  {}",
                    file.display().to_string().replace("\\", "/"),
                    violations_in_line.join("\n  ")
                ));
            }
        }
    }
    violations.sort_unstable();

    assert!(
        violations.is_empty(),
        "platform boundary regression; non-platform src may not contain host branching:\n{}",
        violations.join("\n")
    );
}

fn collect_rs_files(directory: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if path.is_dir() {
            if file_name == "platform" {
                continue;
            }
            collect_rs_files(&path, files)?;
            continue;
        }
        if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    Ok(())
}
