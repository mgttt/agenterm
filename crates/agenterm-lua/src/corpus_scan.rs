//! Directory scanning: recursively find .lua files, check each, produce report.
//!
//! Thin wrapper over the shared driver (`agenterm_script_common::corpus_scan`)
//! — this module's own job is just "filter on `.lua`, drive `LuaEngine::check`".

use std::path::Path;

pub use agenterm_script_common::corpus_scan::{CorpusScanReport, FailedFile};

use crate::LuaEngine;

/// Scan a directory recursively for `.lua` files and check each one.
pub fn scan_directory(dir: &Path) -> Result<CorpusScanReport, String> {
    let engine = LuaEngine::new().map_err(|e| e.to_string())?;
    agenterm_script_common::corpus_scan::scan_directory(dir, &["lua"], |source, _label| {
        engine.check(source).map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use agenterm_script_common::test_support::CorpusScanContract;

    use super::*;

    // The scenarios live in script-common's test_support (they're the
    // shared contract of every engine's corpus-scan wrapper); this block
    // supplies only what's lua-specific: the extension and sources. The
    // recursion scenario is new coverage here — qjs/sql always had it, lua's
    // hand-copied block predated it.
    const CONTRACT: CorpusScanContract<'_> = CorpusScanContract {
        good_a: ("a.lua", "return 42"),
        good_b: ("b.lua", "return 0"),
        bad: ("bad.lua", "return !!"),
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
    fn scan_ignores_non_lua() {
        CONTRACT.assert_ignores_foreign_files(&scan_directory);
    }

    #[test]
    fn scan_recurses_into_subdirectories() {
        CONTRACT.assert_recurses_into_subdirectories(&scan_directory);
    }
}
