//! Directory scanning: recursively find `.sql` files, `check()` each,
//! produce a report. Same shape/purpose as `agenterm_qjs::corpus_scan` /
//! `agenterm_lua::corpus_scan` (batch syntax validation over a tree, no
//! manifest needed — distinct from `check_many`'s explicit-file-list
//! contract).
//!
//! Thin wrapper over the shared driver (`agenterm_script_common::corpus_scan`).

use std::path::Path;

pub use agenterm_script_common::corpus_scan::{CorpusScanReport, FailedFile};

use crate::check;

/// Scan a directory recursively for `.sql` files and check each one.
pub fn scan_directory(dir: &Path) -> Result<CorpusScanReport, String> {
    agenterm_script_common::corpus_scan::scan_directory(dir, &["sql"], |source, label| {
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
        std::fs::write(dir.path().join("a.sql"), "SELECT 1;").unwrap();
        std::fs::write(dir.path().join("b.sql"), "SELECT 2;").unwrap();
        let report = scan_directory(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 2);
        assert_eq!(report.failures, 0);
    }

    #[test]
    fn scan_with_syntax_error() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("ok.sql"), "SELECT 1;").unwrap();
        std::fs::write(dir.path().join("bad.sql"), "SELEC 1 FORM;").unwrap();
        let report = scan_directory(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 2);
        assert_eq!(report.failures, 1);
        assert!(report.failed_files[0].path.contains("bad.sql"));
    }

    #[test]
    fn scan_ignores_non_sql() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("script.sql"), "SELECT 1;").unwrap();
        std::fs::write(dir.path().join("readme.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main(){}").unwrap();
        let report = scan_directory(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 1);
    }

    #[test]
    fn scan_recurses_into_subdirectories() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("lib")).unwrap();
        std::fs::write(dir.path().join("lib/leaf.sql"), "SELECT 1;").unwrap();
        let report = scan_directory(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 1);
    }
}
