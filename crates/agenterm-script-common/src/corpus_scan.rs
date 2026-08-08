//! Generic "walk a directory, `check()` every matching file, report" driver
//! — the shape both `agenterm_lua::corpus_scan` and `agenterm_qjs::corpus_scan`
//! independently implemented line-for-line the same way (extension filter
//! aside). `agenterm_rh::corpus` stays separate on purpose: it's tied to a
//! whole-project transpile pipeline (`transpile_cdylib_with_project`), not
//! a bare syntax check, so it doesn't fit this shape.

use std::path::Path;
use std::time::Instant;

use serde::Serialize;

/// Result of scanning a directory of scripts.
#[derive(Debug, Serialize)]
pub struct CorpusScanReport {
    pub total_scripts: usize,
    pub failures: usize,
    pub duration_ms: u64,
    pub failed_files: Vec<FailedFile>,
}

#[derive(Debug, Serialize)]
pub struct FailedFile {
    pub path: String,
    pub message: String,
}

/// Walk `dir` recursively for files whose extension (case-sensitive, no
/// leading dot) is in `extensions`, calling `check(source, label)` on each
/// — `label` is the file's own path (display form), matching every
/// existing engine's `failed_files[].path` convention.
pub fn scan_directory<F, E>(
    dir: &Path,
    extensions: &[&str],
    mut check: F,
) -> Result<CorpusScanReport, String>
where
    F: FnMut(&str, &str) -> Result<(), E>,
    E: std::fmt::Display,
{
    let started = Instant::now();
    let mut total = 0usize;
    let mut failures = 0usize;
    let mut failed_files = Vec::new();

    let entries = walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.file_type().is_file()
                && entry
                    .path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| extensions.contains(&ext))
        });

    for entry in entries {
        total += 1;
        let path = entry.path();
        let source =
            std::fs::read_to_string(path).map_err(|err| format!("corpus_scan_read: {err}"))?;
        let label = path.to_string_lossy().into_owned();
        if let Err(error) = check(&source, &label) {
            failures += 1;
            failed_files.push(FailedFile {
                path: label,
                message: error.to_string(),
            });
        }
    }

    Ok(CorpusScanReport {
        total_scripts: total,
        failures,
        duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        failed_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn check_ok_unless_bad(source: &str, _label: &str) -> Result<(), String> {
        if source.contains("BAD") {
            Err("contains BAD".to_owned())
        } else {
            Ok(())
        }
    }

    #[test]
    fn scan_empty_dir() {
        let dir = TempDir::new().unwrap();
        let report = scan_directory(dir.path(), &["txt"], check_ok_unless_bad).expect("scan");
        assert_eq!(report.total_scripts, 0);
        assert_eq!(report.failures, 0);
    }

    #[test]
    fn scan_all_green() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.txt"), "ok").unwrap();
        std::fs::write(dir.path().join("b.txt"), "ok").unwrap();
        let report = scan_directory(dir.path(), &["txt"], check_ok_unless_bad).expect("scan");
        assert_eq!(report.total_scripts, 2);
        assert_eq!(report.failures, 0);
    }

    #[test]
    fn scan_reports_failures() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("ok.txt"), "fine").unwrap();
        std::fs::write(dir.path().join("bad.txt"), "BAD").unwrap();
        let report = scan_directory(dir.path(), &["txt"], check_ok_unless_bad).expect("scan");
        assert_eq!(report.total_scripts, 2);
        assert_eq!(report.failures, 1);
        assert!(report.failed_files[0].path.contains("bad.txt"));
    }

    #[test]
    fn scan_filters_by_extension() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("script.txt"), "ok").unwrap();
        std::fs::write(dir.path().join("readme.md"), "ok").unwrap();
        let report = scan_directory(dir.path(), &["txt"], check_ok_unless_bad).expect("scan");
        assert_eq!(report.total_scripts, 1);
    }

    #[test]
    fn scan_accepts_multiple_extensions() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.js"), "ok").unwrap();
        std::fs::write(dir.path().join("b.mjs"), "ok").unwrap();
        std::fs::write(dir.path().join("c.txt"), "ok").unwrap();
        let report = scan_directory(dir.path(), &["js", "mjs"], check_ok_unless_bad).expect("scan");
        assert_eq!(report.total_scripts, 2);
    }

    #[test]
    fn scan_recurses_into_subdirectories() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("lib")).unwrap();
        std::fs::write(dir.path().join("lib/leaf.txt"), "ok").unwrap();
        let report = scan_directory(dir.path(), &["txt"], check_ok_unless_bad).expect("scan");
        assert_eq!(report.total_scripts, 1);
    }
}
