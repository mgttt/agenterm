//! macOS passive WKWebView runtime discovery.

use std::path::PathBuf;

use crate::{contract::webview::SystemWebViewProbe, webview::probe_files};

pub(crate) fn probe_system_webview() -> SystemWebViewProbe {
    probe_files(
        "wkwebview",
        [PathBuf::from(
            "/System/Library/Frameworks/WebKit.framework/WebKit",
        )],
        "system_framework",
        "webkit_framework_not_found",
    )
}
