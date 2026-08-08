//! Directory scanning: recursively find `.js`/`.mjs` files, `check()` each,
//! produce a report. Same shape/purpose as `agenterm_lua::corpus_scan`
//! (batch syntax validation over a tree, no manifest needed — distinct from
//! `check_many`'s explicit-file-list contract).
//!
//! Thin wrapper over the shared driver (`agenterm_script_common::corpus_scan`).

use std::path::Path;

pub use agenterm_script_common::corpus_scan::{CorpusScanReport, FailedFile};

use crate::check;

/// Scan a directory recursively for `.js`/`.mjs` files and check each one.
pub fn scan_directory(dir: &Path) -> Result<CorpusScanReport, String> {
    agenterm_script_common::corpus_scan::scan_directory(dir, &["js", "mjs"], |source, label| {
        check(source, label).map_err(|e| e.to_string())
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
