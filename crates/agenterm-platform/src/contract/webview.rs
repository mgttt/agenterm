//! Platform-neutral facts returned by passive system-WebView discovery.

use std::borrow::Cow;

use crate::CapabilityStatus;

/// Whether the host-provided WebView runtime can be discovered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RuntimePresence {
    Detected,
    Missing,
    Failed,
}

/// A passive probe result. Probing never initializes or downloads a runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct SystemWebViewProbe {
    pub presence: RuntimePresence,
    pub backend: &'static str,
    pub version: Option<String>,
    pub source: Option<String>,
    pub reason: Option<String>,
}

impl SystemWebViewProbe {
    /// Project runtime discovery into the crate-wide typed capability status.
    pub fn capability_status(&self) -> CapabilityStatus {
        match self.presence {
            RuntimePresence::Detected => CapabilityStatus::Available,
            RuntimePresence::Missing => CapabilityStatus::Unsupported {
                reason: Cow::Owned(
                    self.reason
                        .clone()
                        .unwrap_or_else(|| "system-webview-runtime-not-found".to_owned()),
                ),
            },
            RuntimePresence::Failed => CapabilityStatus::Failed {
                code: Cow::Borrowed("system-webview-probe-failed"),
                message: self
                    .reason
                    .clone()
                    .unwrap_or_else(|| "system WebView discovery failed".to_owned()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_failed_probes_remain_distinct() {
        let missing = SystemWebViewProbe {
            presence: RuntimePresence::Missing,
            backend: "test",
            version: None,
            source: None,
            reason: Some("not-installed".to_owned()),
        };
        assert_eq!(
            missing.capability_status(),
            CapabilityStatus::Unsupported {
                reason: Cow::Owned("not-installed".to_owned())
            }
        );

        let failed = SystemWebViewProbe {
            presence: RuntimePresence::Failed,
            backend: "test",
            version: None,
            source: None,
            reason: Some("access-denied".to_owned()),
        };
        assert_eq!(
            failed.capability_status(),
            CapabilityStatus::Failed {
                code: Cow::Borrowed("system-webview-probe-failed"),
                message: "access-denied".to_owned()
            }
        );
    }
}
