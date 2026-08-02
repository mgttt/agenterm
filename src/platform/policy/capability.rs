//! Product capability status policy shared by product modules.
//!
//! agenterm-platform exposes typed capability facts; this table maps them
//! into the product-facing availability surface used by CLI/UI reporting.

use crate::platform::CONTRACT_REVISION;

/// Capability surface an adapter may expose (capability-oriented, not one
/// global `OsLayer` object).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum CapabilityKind {
    Window,
    Input,
    Ime,
    Clipboard,
    Font,
    Screenshot,
    Activation,
    Integration,
}

impl CapabilityKind {
    pub const ALL: [Self; 8] = [
        Self::Window,
        Self::Input,
        Self::Ime,
        Self::Clipboard,
        Self::Font,
        Self::Screenshot,
        Self::Activation,
        Self::Integration,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::Input => "input",
            Self::Ime => "ime",
            Self::Clipboard => "clipboard",
            Self::Font => "font",
            Self::Screenshot => "screenshot",
            Self::Activation => "activation",
            Self::Integration => "integration",
        }
    }
}

/// Explicit availability of a capability. Missing behavior must not be hidden.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CapabilityStatus {
    Available,
    Unsupported { reason: &'static str },
    Failed { code: &'static str, message: String },
}

impl CapabilityStatus {
    fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Available => serde_json::json!({"status": "available"}),
            Self::Unsupported { reason } => {
                serde_json::json!({"status": "unsupported", "reason": reason})
            }
            Self::Failed { code, message } => {
                serde_json::json!({"status": "failed", "code": code, "message": message})
            }
        }
    }
}

pub(crate) fn project_capability_status(
    status: agenterm_platform::CapabilityStatus,
    unsupported_reason: &'static str,
    failed_code: &'static str,
) -> CapabilityStatus {
    match status {
        agenterm_platform::CapabilityStatus::Available => CapabilityStatus::Available,
        agenterm_platform::CapabilityStatus::Unsupported { .. } => CapabilityStatus::Unsupported {
            reason: unsupported_reason,
        },
        agenterm_platform::CapabilityStatus::Failed { message, .. } => CapabilityStatus::Failed {
            code: failed_code,
            message,
        },
        _ => CapabilityStatus::Failed {
            code: failed_code,
            message: "platform capability returned an unknown status".to_owned(),
        },
    }
}

pub(crate) fn platform_info_json() -> serde_json::Value {
    let kind = agenterm_platform::platform_kind();
    let display = agenterm_platform::window::display_backend_facts();

    let capabilities = CapabilityKind::ALL
        .into_iter()
        .map(|capability| {
            let status = match capability {
                CapabilityKind::Window | CapabilityKind::Input => project_capability_status(
                    agenterm_platform::window::capability_status(),
                    "headless-display",
                    "window-failed",
                ),
                CapabilityKind::Ime => {
                    if matches!(kind, agenterm_platform::PlatformKind::Windows) {
                        CapabilityStatus::Unsupported {
                            reason: "ime-preedit-not-yet-adapted",
                        }
                    } else {
                        project_capability_status(
                            agenterm_platform::ime::capability_status(!display.headless),
                            "headless-display",
                            "ime-failed",
                        )
                    }
                }
                CapabilityKind::Clipboard => CapabilityStatus::Available,
                CapabilityKind::Font => project_capability_status(
                    agenterm_platform::font::capability_status(),
                    "font-unsupported",
                    "font-failed",
                ),
                CapabilityKind::Screenshot | CapabilityKind::Activation if display.headless => {
                    CapabilityStatus::Unsupported {
                        reason: "headless-display",
                    }
                }
                CapabilityKind::Screenshot | CapabilityKind::Activation => {
                    CapabilityStatus::Available
                }
                CapabilityKind::Integration => match kind {
                    agenterm_platform::PlatformKind::Windows => CapabilityStatus::Unsupported {
                        reason: "windows-shell-integration-not-yet-declared",
                    },
                    agenterm_platform::PlatformKind::Macos => CapabilityStatus::Unsupported {
                        reason: "signed-macos-app-bundle-pending",
                    },
                    _ => CapabilityStatus::Unsupported {
                        reason: "deferred-slice",
                    },
                },
            };
            (capability.as_str().to_owned(), status.to_json())
        })
        .collect::<serde_json::Map<_, _>>();
    serde_json::json!({
        "contract_revision": CONTRACT_REVISION,
        "kind": match kind {
            agenterm_platform::PlatformKind::Windows => "windows",
            agenterm_platform::PlatformKind::Macos => "macos",
            agenterm_platform::PlatformKind::Linux => "linux",
            _ => "unknown",
        },
        "capabilities": capabilities,
    })
}

#[cfg(test)]
mod tests {
    use super::{CapabilityKind, CapabilityStatus, platform_info_json};
    use crate::platform::CONTRACT_REVISION;

    #[test]
    fn capability_status_keeps_failures_explicit() {
        let unsupported = CapabilityStatus::Unsupported {
            reason: "headless-display",
        };
        let failed = CapabilityStatus::Failed {
            code: "clipboard_unavailable",
            message: "wl-clipboard missing".to_string(),
        };
        assert!(matches!(
            unsupported,
            CapabilityStatus::Unsupported {
                reason: "headless-display"
            }
        ));
        assert!(matches!(
            failed,
            CapabilityStatus::Failed {
                code: "clipboard_unavailable",
                ..
            }
        ));
    }

    #[test]
    fn public_platform_info_reports_every_typed_capability() {
        let info = platform_info_json();
        assert_eq!(info["contract_revision"], CONTRACT_REVISION);
        assert_eq!(
            info["capabilities"].as_object().map(serde_json::Map::len),
            Some(CapabilityKind::ALL.len())
        );
        for capability in CapabilityKind::ALL {
            assert!(info["capabilities"][capability.as_str()]["status"].is_string());
        }
    }
}
