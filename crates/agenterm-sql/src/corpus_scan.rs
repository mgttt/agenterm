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
    use agenterm_script_common::test_support::CorpusScanContract;

    use super::*;

    // The scenarios live in script-common's test_support (they're the
    // shared contract of every engine's corpus-scan wrapper); this block
    // supplies only what's sql-specific: the extension and sources.
    const CONTRACT: CorpusScanContract<'_> = CorpusScanContract {
        good_a: ("a.sql", "SELECT 1;"),
        good_b: ("b.sql", "SELECT 2;"),
        bad: ("bad.sql", "SELEC 1 FORM;"),
    };

    #[test]
    fn scan_empty_dir() {
        CONTRACT.assert_empty_dir(&scan_directory);
    }

    #[test]
    fn scan_all_green() {
        CONTRACT.assert_all_green(&scan_directory);
    }

    #[test]
    fn scan_with_syntax_error() {
        CONTRACT.assert_syntax_error_reported(&scan_directory);
    }

    #[test]
    fn scan_ignores_non_sql() {
        CONTRACT.assert_ignores_foreign_files(&scan_directory);
    }

    #[test]
    fn scan_recurses_into_subdirectories() {
        CONTRACT.assert_recurses_into_subdirectories(&scan_directory);
    }
}
