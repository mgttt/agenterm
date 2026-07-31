//! OS-neutral facts returned by passive system-WebView discovery.

use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimePresence {
    Detected,
    Missing,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SystemWebViewProbe {
    pub(crate) presence: RuntimePresence,
    pub(crate) backend: &'static str,
    pub(crate) version: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) reason: Option<String>,
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

#[allow(dead_code)] // Reached by Linux/macOS selected adapters, not Windows.
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

#[allow(dead_code)] // Used only by the Linux/macOS file probe above.
fn version_from_path(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split(".so.").nth(1))
        .map(str::to_owned)
}

pub(crate) fn unavailable_or_failed(
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
