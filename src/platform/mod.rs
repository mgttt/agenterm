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

pub(crate) mod adapters;
pub(crate) mod filesystem;
pub(crate) mod policy;
#[allow(dead_code, unused_imports)]
pub(crate) use policy::input::{
    is_primary_shortcut_via_meta, primary_text_field_shortcut_modifiers,
    terminal_shortcut_empty_copy_action_is_suppressed,
};

#[allow(unused_imports)]
pub(crate) use policy::host::{
    is_macos_host, is_unix_host, is_windows_host, shell_command_for_host,
};
pub(crate) use policy::paths::{
    default_audit_path, default_workspace_path, instance_registry_directory_root,
    ipc_default_workspace_path, ipc_default_workspace_path_for,
    script_worker_default_executable_name, settings_root_path, terminal_default_font_size,
    workspace_instance_scope,
};
pub(crate) use policy::runtime::hosted_script_worker_available;
#[allow(unused_imports)]
pub(crate) use policy::script_http::{
    script_http_tls_config, script_http_tls_provider, script_http_tls_root_certs_are_expected,
};
#[allow(unused_imports)]
pub(crate) use policy::workspace::{WorkspaceLayoutKind, workspace_layout_kind};

pub(crate) use agenterm_platform::console_interrupt::{
    ConsoleInterruptIgnoreGuard, ConsoleInterruptObserver,
};
pub(crate) use agenterm_platform::console_line_editor::{
    ConsoleKey, ConsoleLineEditor, LineBuffer, LineHistory,
};
pub use filesystem::{
    is_direct_directory, is_direct_file, metadata_is_link_like, replace_file, sync_parent,
};

pub fn install_console_interrupt_ignore_guard() -> anyhow::Result<ConsoleInterruptIgnoreGuard> {
    ConsoleInterruptIgnoreGuard::install().map_err(|error| anyhow::anyhow!("{error}"))
}

pub fn install_console_interrupt_observer() -> anyhow::Result<ConsoleInterruptObserver> {
    ConsoleInterruptObserver::install().map_err(|error| anyhow::anyhow!("{error}"))
}

pub fn enter_console_line_editor() -> anyhow::Result<ConsoleLineEditor> {
    ConsoleLineEditor::enter().map_err(|error| anyhow::anyhow!("{error}"))
}

// Platform Facade services. Product modules consume these typed services;
// their selected OS implementations stay private to this boundary.
// Several services are staged ahead of their final product-caller migration;
// keeping their typed contracts compiled on every target is intentional.
#[cfg(test)]
mod boundary_tests;
pub(crate) mod contract;
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

pub use agenterm_platform::input::{KeyClassification, ModifierState};
#[allow(unused_imports)]
pub(crate) use policy::capability::{CapabilityKind, CapabilityStatus, platform_info_json};
pub(crate) use policy::ipc::ipc_default_native_endpoint;

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
            ("new-tab", crate::frontend::action::NEW_TAB),
            ("toggle-tabs", crate::frontend::action::TOGGLE_TABS),
            (
                "open-control-center",
                crate::frontend::action::OPEN_CONTROL_CENTER,
            ),
            ("open-settings", crate::frontend::action::OPEN_SETTINGS),
            ("toggle-locale", crate::frontend::action::TOGGLE_LOCALE),
            ("font-decrease", crate::frontend::action::FONT_DECREASE),
            ("font-increase", crate::frontend::action::FONT_INCREASE),
        ];
        for (want, got) in expected {
            assert_eq!(got, want);
        }
    }

    #[test]
    fn toolbar_action_order_matches_prd_geometry() {
        assert_eq!(
            crate::frontend::action::TOOLBAR_ACTION_ORDER,
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
        for action_id in crate::frontend::action::TOOLBAR_ACTION_ORDER {
            assert!(crate::frontend::action::is_toolbar_action_id(action_id));
        }
        assert!(!crate::frontend::action::is_toolbar_action_id(
            "adapter-local-action"
        ));
    }

    #[test]
    fn native_toolbar_order_matches_contract() {
        use crate::frontend::toolbar::NativeToolbarHit;
        assert_eq!(
            NativeToolbarHit::ORDER.map(NativeToolbarHit::action_id),
            crate::frontend::action::TOOLBAR_ACTION_ORDER
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
    fn primary_shortcut_policy_is_internal_consistent() {
        let by_meta = is_primary_shortcut_via_meta();
        let modifiers = primary_text_field_shortcut_modifiers();
        if by_meta {
            assert!(modifiers.meta);
            assert!(!modifiers.control);
            assert!(terminal_shortcut_empty_copy_action_is_suppressed());
        } else {
            assert!(modifiers.control);
            assert!(!modifiers.meta);
            assert!(!terminal_shortcut_empty_copy_action_is_suppressed());
        }
    }

    #[test]
    fn primary_shortcut_policy_matches_runtime_kind() {
        assert_eq!(
            is_primary_shortcut_via_meta(),
            matches!(
                agenterm_platform::platform_kind(),
                agenterm_platform::PlatformKind::Macos
            )
        );
    }

    #[test]
    fn control_center_screenshot_strategy_matches_runtime_kind() {
        assert_eq!(
            crate::platform::policy::control_center::screenshot_strategy(),
            match agenterm_platform::platform_kind() {
                agenterm_platform::PlatformKind::Windows => {
                    crate::platform::policy::control_center::ScreenshotStrategy::DirectNativeWindow
                }
                agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos => {
                    crate::platform::policy::control_center::ScreenshotStrategy::RendererRequest
                }
                _ => crate::platform::policy::control_center::ScreenshotStrategy::Unsupported,
            }
        );
    }

    #[test]
    fn hosted_script_worker_available_tracks_host_runtime() {
        assert_eq!(
            hosted_script_worker_available(),
            matches!(
                agenterm_platform::platform_kind(),
                agenterm_platform::PlatformKind::Windows
            )
        );
    }
}
