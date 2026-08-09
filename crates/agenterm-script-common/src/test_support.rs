//! Shared contract assertions for engine test suites.
//!
//! lua/qjs/sql each carried a structurally identical `#[cfg(test)]` block
//! (~50 lines) asserting the same scenarios against their own
//! `corpus_scan::scan_directory` wrapper: empty dir, all green, one syntax
//! error, foreign files ignored, subdirectory recursion. Those scenarios
//! ARE the contract of [`crate::corpus_scan::scan_directory`] as observed
//! through an engine wrapper — so they live here once, and each engine
//! keeps one-line `#[test]`s per scenario (granular failures preserved).
//!
//! Behind the `test-support` feature so `tempfile` stays a dev-only cost:
//! engines enable it from `[dev-dependencies]` only.

use std::path::Path;

use crate::corpus_scan::CorpusScanReport;

/// Per-engine inputs for the corpus-scan contract: two sources that must
/// check green (second one may use an alternate extension — qjs uses
/// `.mjs`), and one that must fail the engine's `check`.
pub struct CorpusScanContract<'a> {
    /// `(file_name, source)` — must check green. Its extension is the
    /// engine's primary one; also used by the foreign-file and recursion
    /// scenarios.
    pub good_a: (&'a str, &'a str),
    /// `(file_name, source)` — must check green; exercises a second
    /// accepted extension where the engine has one.
    pub good_b: (&'a str, &'a str),
    /// `(file_name, source)` — must fail the engine's `check`.
    pub bad: (&'a str, &'a str),
}

type ScanFn<'f> = &'f dyn Fn(&Path) -> Result<CorpusScanReport, String>;

impl CorpusScanContract<'_> {
    pub fn assert_empty_dir(&self, scan: ScanFn) {
        let dir = tempfile::TempDir::new().unwrap();
        let report = scan(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 0);
        assert_eq!(report.failures, 0);
    }

    pub fn assert_all_green(&self, scan: ScanFn) {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join(self.good_a.0), self.good_a.1).unwrap();
        std::fs::write(dir.path().join(self.good_b.0), self.good_b.1).unwrap();
        let report = scan(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 2);
        assert_eq!(report.failures, 0);
    }

    pub fn assert_syntax_error_reported(&self, scan: ScanFn) {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join(self.good_a.0), self.good_a.1).unwrap();
        std::fs::write(dir.path().join(self.bad.0), self.bad.1).unwrap();
        let report = scan(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 2);
        assert_eq!(report.failures, 1);
        assert!(
            report.failed_files[0].path.contains(self.bad.0),
            "failed path {:?} should name {:?}",
            report.failed_files[0].path,
            self.bad.0
        );
    }

    pub fn assert_ignores_foreign_files(&self, scan: ScanFn) {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join(self.good_a.0), self.good_a.1).unwrap();
        std::fs::write(dir.path().join("readme.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main(){}").unwrap();
        let report = scan(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 1);
    }

    pub fn assert_recurses_into_subdirectories(&self, scan: ScanFn) {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("lib")).unwrap();
        std::fs::write(dir.path().join("lib").join(self.good_a.0), self.good_a.1).unwrap();
        let report = scan(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 1);
    }
}
