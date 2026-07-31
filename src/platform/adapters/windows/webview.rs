//! Windows passive WebView2 runtime discovery.

use std::path::PathBuf;

use crate::platform::contract::webview::{SystemWebViewProbe, probe_version_directories};

pub(crate) fn probe_system_webview() -> SystemWebViewProbe {
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
