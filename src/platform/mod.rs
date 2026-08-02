//! Native platform abstraction contracts (`prd/PRD_02_20_native_platform.md`).
//!
//! **Contract revision 3** freezes normalized product-action identities, key
//! classification, capability status, display-backend facts, and validated
//! window lifecycle/scale/geometry semantics with table-driven unit tests.
//!
//! Ownership (PRD parallel rules):
//! - primary owns this file, shared semantics, Windows adaptation, and final
//!   integration;
//! - macOS / Linux agents own only their adapter trees and native evidence;
//! - adapter agents must request a contract change instead of editing this
//!   file's semantics.
//!
//! Adapter modules are declared only when the corresponding tree exists.

/// Frozen shared-contract revision implemented by this module.
#[allow(dead_code)]
pub const CONTRACT_REVISION: u32 = 3;

pub(crate) mod toolbar;
pub(crate) mod window;

// Platform Facade services. Product modules consume these typed services;
// their selected OS implementations stay private to this boundary.
// Several services are staged ahead of their final product-caller migration;
// keeping their typed contracts compiled on every target is intentional.
#[cfg(test)]
mod boundary_tests;
pub(crate) mod contract;
#[allow(dead_code)]
pub(crate) mod control_center;
#[allow(dead_code)]
pub(crate) mod ipc;
#[allow(dead_code)]
pub(crate) mod paths;
#[allow(dead_code)]
pub(crate) mod process;
#[allow(dead_code)]
pub(crate) mod runtime;
#[allow(dead_code)]
pub(crate) mod script_http;
#[allow(dead_code)]
pub(crate) mod services;
#[allow(dead_code)]
pub(crate) mod webview;

/// Stable product action identities consumed by toolbar / shortcut surfaces.
///
/// Win32 control IDs, winit events, and HTML elements remain adapter details.
pub mod action {
    pub const NEW_TAB: &str = "new-tab";
    pub const TOGGLE_TABS: &str = "toggle-tabs";
    pub const OPEN_CONTROL_CENTER: &str = "open-control-center";
    pub const OPEN_SETTINGS: &str = "open-settings";
    pub const TOGGLE_LOCALE: &str = "toggle-locale";
    pub const FONT_DECREASE: &str = "font-decrease";
    pub const FONT_INCREASE: &str = "font-increase";

    /// Canonical left-to-right toolbar order. Every adapter `ORDER` must match.
    pub const TOOLBAR_ACTION_ORDER: [&str; 7] = [
        TOGGLE_TABS,
        NEW_TAB,
        OPEN_CONTROL_CENTER,
        OPEN_SETTINGS,
        TOGGLE_LOCALE,
        FONT_DECREASE,
        FONT_INCREASE,
    ];

    /// Reject adapter-local or stale identities before product dispatch.
    pub fn is_toolbar_action_id(action_id: &str) -> bool {
        TOOLBAR_ACTION_ORDER.contains(&action_id)
    }
}

/// Which operating-system adapter identity is speaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum PlatformKind {
    Windows,
    Macos,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub(crate) enum FrontendHost {
    Windows,
    Unix,
    Unsupported,
}

pub(crate) fn frontend_host() -> FrontendHost {
    match agenterm_platform::platform_kind() {
        agenterm_platform::PlatformKind::Windows => FrontendHost::Windows,
        agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos => {
            FrontendHost::Unix
        }
        _ => FrontendHost::Unsupported,
    }
}

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

pub use agenterm_platform::input::{KeyClassification, ModifierState};

/// Display / window-system discovery facts (not auth).
///
/// Linux populates X11/Wayland; other platforms leave those flags false and
/// report headless through their own window capability diagnostics.
#[allow(unused_imports)]
pub use agenterm_platform::window::DisplayBackendFacts;

#[cfg(test)]
pub use agenterm_platform::contract::input::classify_key_press;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_revision_is_frozen_at_three() {
        assert_eq!(CONTRACT_REVISION, 3);
    }

    #[test]
    fn product_action_identities_match_prd_examples() {
        let expected = [
            ("new-tab", action::NEW_TAB),
            ("toggle-tabs", action::TOGGLE_TABS),
            ("open-control-center", action::OPEN_CONTROL_CENTER),
            ("open-settings", action::OPEN_SETTINGS),
            ("toggle-locale", action::TOGGLE_LOCALE),
            ("font-decrease", action::FONT_DECREASE),
            ("font-increase", action::FONT_INCREASE),
        ];
        for (want, got) in expected {
            assert_eq!(got, want);
        }
    }

    #[test]
    fn toolbar_action_order_matches_prd_geometry() {
        assert_eq!(
            action::TOOLBAR_ACTION_ORDER,
            [
                "toggle-tabs",
                "new-tab",
                "open-control-center",
                "open-settings",
                "toggle-locale",
                "font-decrease",
                "font-increase",
            ]
        );
        for action_id in action::TOOLBAR_ACTION_ORDER {
            assert!(action::is_toolbar_action_id(action_id));
        }
        assert!(!action::is_toolbar_action_id("adapter-local-action"));
    }

    #[test]
    fn native_toolbar_order_matches_contract() {
        use crate::platform::toolbar::NativeToolbarHit;
        assert_eq!(
            NativeToolbarHit::ORDER.map(NativeToolbarHit::action_id),
            action::TOOLBAR_ACTION_ORDER
        );
    }

    #[test]
    fn key_classification_table() {
        let none = ModifierState::empty();
        let ctrl = ModifierState {
            control: true,
            ..ModifierState::empty()
        };
        let shift = ModifierState {
            shift: true,
            ..ModifierState::empty()
        };

        let cases = [
            (
                "primary shortcut stays distinct from text",
                ctrl,
                Some("c"),
                None,
                Some("c"),
                KeyClassification::Shortcut {
                    key: "c".to_string(),
                    modifiers: ctrl,
                },
            ),
            (
                "shift punctuation uses committed text",
                shift,
                Some("!"),
                None,
                Some("!"),
                KeyClassification::TextCommit("!".to_string()),
            ),
            (
                "space without shortcut is text commit",
                none,
                None,
                Some("Space"),
                Some(" "),
                KeyClassification::TextCommit(" ".to_string()),
            ),
            (
                "named control without text is control key",
                none,
                None,
                Some("Escape"),
                None,
                KeyClassification::ControlKey {
                    name: "Escape".to_string(),
                    modifiers: none,
                },
            ),
            (
                "native committed text wins over logical character",
                none,
                Some("a"),
                None,
                Some("à"),
                KeyClassification::TextCommit("à".to_string()),
            ),
        ];

        for (label, modifiers, logical, named, committed, want) in cases {
            assert_eq!(
                classify_key_press(
                    modifiers.control_or_meta(),
                    modifiers,
                    logical,
                    named,
                    committed
                ),
                want,
                "{label}"
            );
        }
    }

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
