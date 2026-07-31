//! Compatibility projection for passive system-WebView discovery.
//!
//! Product modules consume this facade only. Runtime-specific filesystem
//! probing is selected by the platform service and implemented by adapters.

pub(crate) use crate::platform::contract::webview::{RuntimePresence, SystemWebViewProbe};

/// Inspect the host-provided WebView runtime without initializing it.
pub(crate) fn probe_system_webview() -> SystemWebViewProbe {
    crate::platform::services::webview::probe_system_webview()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_returns_a_truthful_backend() {
        let facts = probe_system_webview();
        assert!(!facts.backend.is_empty());
    }
}
