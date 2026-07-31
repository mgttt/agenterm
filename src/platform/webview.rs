//! Passive system-WebView discovery behind the platform facade.
//!
//! Product modules consume [`probe_system_webview`] and never select an OS
//! implementation themselves.  Adapters only inspect local runtime facts;
//! they do not initialize a renderer, download a runtime, or start a process.

use std::{fs, path::PathBuf};

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

/// Inspect the host-provided WebView runtime without initializing it.
pub(crate) fn probe_system_webview() -> SystemWebViewProbe {
    selected::probe()
}

#[cfg(target_os = "windows")]
mod selected {
    use super::*;

    pub(super) fn probe() -> SystemWebViewProbe {
        let roots = ["LOCALAPPDATA", "PROGRAMFILES", "PROGRAMFILES(X86)"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .map(|root| {
                root.join("Microsoft")
                    .join("EdgeWebView")
                    .join("Application")
            });
        probe_version_directories("webview2", roots, "webview2_runtime_not_found")
    }
}

#[cfg(target_os = "macos")]
mod selected {
    use super::*;

    pub(super) fn probe() -> SystemWebViewProbe {
        probe_files(
            "wkwebview",
            [PathBuf::from(
                "/System/Library/Frameworks/WebKit.framework/WebKit",
            )],
            "system_framework",
            "webkit_framework_not_found",
        )
    }
}

#[cfg(target_os = "linux")]
mod selected {
    use super::*;

    pub(super) fn probe() -> SystemWebViewProbe {
        const DIRECTORIES: [&str; 6] = [
            "/usr/lib",
            "/usr/lib64",
            "/usr/lib/x86_64-linux-gnu",
            "/usr/lib/aarch64-linux-gnu",
            "/lib",
            "/lib64",
        ];
        const NAMES: [&str; 3] = [
            "libwebkit2gtk-4.1.so.0",
            "libwebkit2gtk-4.0.so.37",
            "libwebkit2gtk-4.0.so.0",
        ];
        let paths = DIRECTORIES.into_iter().flat_map(|directory| {
            NAMES
                .into_iter()
                .map(move |name| PathBuf::from(directory).join(name))
        });
        probe_files(
            "webkitgtk",
            paths,
            "system_library",
            "webkitgtk_runtime_not_found",
        )
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod selected {
    use super::*;

    pub(super) fn probe() -> SystemWebViewProbe {
        SystemWebViewProbe {
            presence: RuntimePresence::Missing,
            backend: "none",
            version: None,
            source: None,
            reason: Some("platform_backend_not_defined".to_owned()),
        }
    }
}

#[cfg(target_os = "windows")]
fn probe_version_directories(
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

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn probe_files(
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

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn version_from_path(path: &std::path::Path) -> Option<String> {
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
    fn selected_adapter_returns_a_truthful_backend() {
        let facts = probe_system_webview();
        #[cfg(target_os = "windows")]
        assert_eq!(facts.backend, "webview2");
        #[cfg(target_os = "macos")]
        assert_eq!(facts.backend, "wkwebview");
        #[cfg(target_os = "linux")]
        assert_eq!(facts.backend, "webkitgtk");
        assert!(!facts.backend.is_empty());
    }
}
