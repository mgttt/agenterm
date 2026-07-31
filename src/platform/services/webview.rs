//! OS-neutral passive system-WebView discovery service.

use crate::platform::{contract::webview::SystemWebViewProbe, selected};

pub(crate) fn probe_system_webview() -> SystemWebViewProbe {
    selected::webview::probe_system_webview()
}
