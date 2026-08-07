//! Directory scanning: recursively find .lua files, check each, produce report.

use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use crate::LuaEngine;

/// Result of scanning a directory of Lua scripts.
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

/// Scan a directory recursively for `.lua` files and check each one.
pub fn scan_directory(dir: &Path) -> Result<CorpusScanReport, String> {
    let started = Instant::now();
    let engine = LuaEngine::new().map_err(|e| e.to_string())?;
    let mut total = 0usize;
    let mut failures = 0usize;
    let mut failed_files = Vec::new();

    let entries = walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "lua"));

    for entry in entries {
        total += 1;
        let path = entry.path();
        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("corpus_scan_read: {e}"))?;
        if let Err(e) = engine.check(&source) {
            failures += 1;
            failed_files.push(FailedFile {
                path: path.to_string_lossy().into_owned(),
                message: e.to_string(),
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
