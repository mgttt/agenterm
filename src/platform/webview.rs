//! AgenTerm compatibility projection for passive system-WebView discovery.

pub(crate) use agenterm_platform::webview::RuntimePresence;

pub(crate) fn probe_system_webview() -> agenterm_platform::webview::SystemWebViewProbe {
    agenterm_platform::webview::probe_system_webview()
}
