//! Passive discovery of the host-provided system WebView runtime.
//!
//! Discovery only inspects conventional runtime paths. It does not initialize,
//! install, update, or download a WebView implementation.

use std::{
    fs,
    path::{Path, PathBuf},
};

pub use crate::contract::webview::{RuntimePresence, SystemWebViewProbe};

/// Inspect the selected platform's system WebView runtime.
pub fn probe_system_webview() -> SystemWebViewProbe {
    crate::selected::webview::probe_system_webview()
}

pub(crate) fn probe_version_directories(
    backend: &'static str,
    roots: impl IntoIterator<Item = PathBuf>,
    missing_reason: &'static str,
) -> SystemWebViewProbe {
    let mut failure = None;
    for root in roots {
        match fs::read_dir(&root) {
            Ok(entries) => {
                let newest = entries
                    .filter_map(Result::ok)
                    .filter(|entry| entry.path().is_dir())
                    .max_by_key(|entry| entry.file_name());
                if let Some(entry) = newest {
                    return SystemWebViewProbe {
                        presence: RuntimePresence::Detected,
                        backend,
                        version: Some(entry.file_name().to_string_lossy().into_owned()),
                        source: Some(entry.path().display().to_string()),
                        reason: None,
                    };
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => failure = Some(format!("probe_failed:{error}")),
        }
    }
    unavailable_or_failed(backend, missing_reason, failure)
}

#[allow(dead_code)] // Selected by Linux/macOS adapters; Windows still tests the pure probe.
pub(crate) fn probe_files(
    backend: &'static str,
    paths: impl IntoIterator<Item = PathBuf>,
    source_kind: &'static str,
    missing_reason: &'static str,
) -> SystemWebViewProbe {
    let mut failure = None;
    for path in paths {
        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => {
                return SystemWebViewProbe {
                    presence: RuntimePresence::Detected,
                    backend,
                    version: version_from_path(&path),
                    source: Some(format!("{source_kind}:{}", path.display())),
                    reason: None,
                };
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => failure = Some(format!("probe_failed:{error}")),
        }
    }
    unavailable_or_failed(backend, missing_reason, failure)
}

#[allow(dead_code)] // Used by the cross-target file probe above.
fn version_from_path(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split(".so.").nth(1))
        .map(str::to_owned)
}

fn unavailable_or_failed(
    backend: &'static str,
    missing_reason: &'static str,
    failure: Option<String>,
) -> SystemWebViewProbe {
    SystemWebViewProbe {
        presence: if failure.is_some() {
            RuntimePresence::Failed
        } else {
            RuntimePresence::Missing
        },
        backend,
        version: None,
        source: None,
        reason: failure.or_else(|| Some(missing_reason.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_probe_is_explicitly_unsupported() {
        let path = std::env::temp_dir().join(format!(
            "agenterm-platform-webview-missing-{}",
            std::process::id()
        ));
        let probe = probe_files("test", [path], "test_path", "runtime-not-found");
        assert_eq!(probe.presence, RuntimePresence::Missing);
        assert_eq!(probe.reason.as_deref(), Some("runtime-not-found"));
    }

    #[test]
    fn selected_probe_names_its_backend() {
        assert!(!probe_system_webview().backend.is_empty());
    }
}
