//! Directory scanning: recursively find `.js`/`.mjs` files, `check()` each,
//! produce a report. Same shape/purpose as `agenterm_lua::corpus_scan`
//! (batch syntax validation over a tree, no manifest needed — distinct from
//! `check_many`'s explicit-file-list contract).

use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use crate::check;

/// Result of scanning a directory of qjs scripts.
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

/// Scan a directory recursively for `.js`/`.mjs` files and check each one.
pub fn scan_directory(dir: &Path) -> Result<CorpusScanReport, String> {
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
                    .is_some_and(|ext| ext == "js" || ext == "mjs")
        });

    for entry in entries {
        total += 1;
        let path = entry.path();
        let source =
            std::fs::read_to_string(path).map_err(|e| format!("corpus_scan_read: {e}"))?;
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

    #[test]
    fn scan_empty_dir() {
        let dir = TempDir::new().unwrap();
        let report = scan_directory(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 0);
        assert_eq!(report.failures, 0);
    }

    #[test]
    fn scan_all_green() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.js"), "function entry() { return 42; }").unwrap();
        std::fs::write(dir.path().join("b.mjs"), "function entry() { return 0; }").unwrap();
        let report = scan_directory(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 2);
        assert_eq!(report.failures, 0);
    }

    #[test]
    fn scan_with_syntax_error() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("ok.js"), "function entry() { return 1; }").unwrap();
        std::fs::write(dir.path().join("bad.js"), "this is not valid js (((").unwrap();
        let report = scan_directory(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 2);
        assert_eq!(report.failures, 1);
        assert!(report.failed_files[0].path.contains("bad.js"));
    }

    #[test]
    fn scan_ignores_non_js() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("script.js"), "function entry() { return 0; }").unwrap();
        std::fs::write(dir.path().join("readme.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main(){}").unwrap();
        let report = scan_directory(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 1);
    }

    #[test]
    fn scan_recurses_into_subdirectories() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("lib")).unwrap();
        std::fs::write(dir.path().join("lib/leaf.js"), "function entry() { return 1; }").unwrap();
        let report = scan_directory(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 1);
    }
}
