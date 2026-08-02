use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN_HOST_BRANCH_MARKERS: [(&str, &str); 7] = [
    ("is_windows_host(", "host predicate usage"),
    ("is_unix_host(", "host predicate usage"),
    ("platform_kind(", "platform kind dispatch"),
    ("cfg!(windows)", "compile-time windows branch"),
    ("cfg!(unix)", "compile-time unix branch"),
    ("#[cfg(windows)]", "compile-time windows branch"),
    ("#[cfg(unix)]", "compile-time unix branch"),
];

#[test]
fn non_platform_src_has_no_runtime_host_branching_references() {
    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs_files(&src_root, &mut files).expect("collect src tree");

    let mut violations = Vec::new();
    for file in files {
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
            if line.contains("PlatformKind::") && !line.contains("crate::platform::platform::PlatformKind")
                && !line.contains("agenterm_platform::PlatformKind")
            {
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
        let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
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
