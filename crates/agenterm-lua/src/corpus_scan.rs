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
        std::fs::write(dir.path().join("a.lua"), "return 42").unwrap();
        std::fs::write(dir.path().join("b.lua"), "return 0").unwrap();
        let report = scan_directory(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 2);
        assert_eq!(report.failures, 0);
    }

    #[test]
    fn scan_with_syntax_error() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("ok.lua"), "return 1").unwrap();
        std::fs::write(dir.path().join("bad.lua"), "return !!").unwrap();
        let report = scan_directory(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 2);
        assert_eq!(report.failures, 1);
        assert!(report.failed_files[0].path.contains("bad.lua"));
    }

    #[test]
    fn scan_ignores_non_lua() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("script.lua"), "return 0").unwrap();
        std::fs::write(dir.path().join("readme.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main(){}").unwrap();
        let report = scan_directory(dir.path()).expect("scan");
        assert_eq!(report.total_scripts, 1);
    }
}
