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

pub(crate) use agenterm_platform::console_interrupt::{
    ConsoleInterruptIgnoreGuard, ConsoleInterruptObserver,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub(crate) enum WorkspaceLayoutKind {
    WindowsFlat,
    DirectoryByScope,
}

#[allow(dead_code)]
pub(crate) fn workspace_layout_kind() -> WorkspaceLayoutKind {
    if is_windows_host() {
        WorkspaceLayoutKind::WindowsFlat
    } else {
        WorkspaceLayoutKind::DirectoryByScope
    }
}

#[allow(dead_code)]
pub(crate) fn script_process_test_host_supported() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Windows
            | agenterm_platform::PlatformKind::Linux
            | agenterm_platform::PlatformKind::Macos
    )
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

#[allow(dead_code)]
pub(crate) fn primary_text_field_shortcut_modifiers() -> ModifierState {
    if matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Macos
    ) {
        ModifierState {
            control: false,
            shift: false,
            alt: false,
            meta: true,
        }
    } else {
        ModifierState {
            control: true,
            shift: false,
            alt: false,
            meta: false,
        }
    }
}

#[allow(dead_code)]
pub(crate) fn is_primary_shortcut_via_meta() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Macos
    )
}

#[allow(dead_code)]
pub(crate) fn terminal_shortcut_empty_copy_action_is_suppressed() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Macos
    )
}

pub(crate) fn control_center_screenshot_strategy()
-> crate::platform::control_center::ScreenshotStrategy {
    match agenterm_platform::platform_kind() {
        agenterm_platform::PlatformKind::Windows => {
            crate::platform::control_center::ScreenshotStrategy::DirectNativeWindow
        }
        agenterm_platform::PlatformKind::Macos | agenterm_platform::PlatformKind::Linux => {
            crate::platform::control_center::ScreenshotStrategy::RendererRequest
        }
        _ => crate::platform::control_center::ScreenshotStrategy::Unsupported,
    }
}

pub(crate) fn hosted_script_worker_available() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Windows
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub(crate) enum AtomicPathSemantics {
    VerbatimLongPath,
    CanonicalSafe,
}

#[allow(dead_code)]
pub(crate) fn atomic_path_semantics() -> AtomicPathSemantics {
    if is_windows_host() {
        AtomicPathSemantics::VerbatimLongPath
    } else {
        AtomicPathSemantics::CanonicalSafe
    }
}

#[allow(dead_code)]
pub(crate) fn long_running_process_command_fixture() -> (&'static str, &'static [&'static str]) {
    if is_windows_host() {
        ("ping.exe", &["-n", "30", "127.0.0.1", ">nul"])
    } else {
        ("sleep", &["30"])
    }
}

#[allow(dead_code)]
pub(crate) fn long_running_process_command_timeout() -> (&'static str, &'static [&'static str]) {
    if is_windows_host() {
        ("ping", &["-n", "6", "127.0.0.1"])
    } else {
        ("sleep", &["5"])
    }
}

pub(crate) fn is_windows_host() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Windows
    )
}

#[allow(dead_code)]
pub(crate) fn shell_command_for_host<'a>(
    windows_command: &'a str,
    unix_command: &'a str,
) -> &'a str {
    if is_windows_host() {
        windows_command
    } else {
        unix_command
    }
}

pub(crate) fn is_macos_host() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Macos
    )
}

pub(crate) fn is_unix_host() -> bool {
    matches!(
        agenterm_platform::platform_kind(),
        agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos
    )
}

pub(crate) fn product_directory_name() -> &'static str {
    if is_windows_host() {
        "AgenTerm"
    } else {
        "agenterm"
    }
}

pub(crate) const fn script_worker_default_executable_name() -> &'static str {
    "agenterm-rhai"
}

pub(crate) fn terminal_default_font_size() -> u16 {
    if is_macos_host() { 14 } else { 12 }
}

pub(crate) fn default_audit_path() -> std::path::PathBuf {
    if is_windows_host() {
        std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(product_directory_name())
            .join("script-audit.jsonl")
    } else if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        std::path::PathBuf::from(path)
            .join(product_directory_name())
            .join("script-audit.jsonl")
    } else if let Some(home) = std::env::var_os("HOME") {
        std::path::PathBuf::from(home)
            .join(".local")
            .join("share")
            .join(product_directory_name())
            .join("script-audit.jsonl")
    } else {
        std::env::temp_dir().join("agenterm-rhai-audit.jsonl")
    }
}

fn host_directories() -> agenterm_platform::filesystem::HostDirectories {
    agenterm_platform::filesystem::host_directories().unwrap_or_else(|_| {
        let fallback = std::env::temp_dir();
        agenterm_platform::filesystem::HostDirectories {
            config: fallback.clone(),
            local_data: fallback,
        }
    })
}

pub(crate) fn default_workspace_path() -> std::path::PathBuf {
    if is_windows_host() {
        local_data_root_for_product_directory(&host_directories()).join("workspace.json")
    } else {
        let scope = std::env::var("AGENTERM_INSTANCE")
            .ok()
            .and_then(|value| {
                value
                    .parse::<crate::platform::contract::ipc::LogicalInstance>()
                    .ok()
            })
            .and_then(|instance| {
                crate::platform::contract::ipc::ServerScopeId::current(&instance).ok()
            });
        scope
            .map(|scope_id| crate::platform::ipc::default_workspace_path(&scope_id))
            .unwrap_or_else(|| {
                local_data_root_for_product_directory(&host_directories())
                    .join("workspaces")
                    .join("main.json")
            })
    }
}

pub(crate) fn settings_root_path() -> std::path::PathBuf {
    let directories = host_directories();
    if is_windows_host() {
        local_data_root_for_product_directory(&directories)
    } else {
        config_root_for_product_directory(&directories)
    }
}

pub(crate) fn workspace_instance_scope() -> Option<crate::platform::contract::ipc::ServerScopeId> {
    if !is_unix_host() {
        return None;
    }
    std::env::var("AGENTERM_INSTANCE")
        .ok()
        .and_then(|value| {
            value
                .parse::<crate::platform::contract::ipc::LogicalInstance>()
                .ok()
        })
        .and_then(|instance| crate::platform::contract::ipc::ServerScopeId::current(&instance).ok())
}

pub(crate) fn instance_registry_directory_root() -> std::path::PathBuf {
    host_directories().local_data.join(product_directory_name())
}

pub(crate) fn script_http_tls_config() -> Result<ureq::tls::TlsConfig, &'static str> {
    let (provider, roots) = if is_windows_host() {
        (
            ureq::tls::TlsProvider::NativeTls,
            ureq::tls::RootCerts::PlatformVerifier,
        )
    } else if is_unix_host() {
        (ureq::tls::TlsProvider::Rustls, ureq::tls::RootCerts::WebPki)
    } else {
        return Err("http_tls_backend_unsupported");
    };
    Ok(ureq::tls::TlsConfig::builder()
        .provider(provider)
        .root_certs(roots)
        .build())
}

#[allow(dead_code)]
pub(crate) fn script_http_tls_provider() -> ureq::tls::TlsProvider {
    if is_windows_host() {
        ureq::tls::TlsProvider::NativeTls
    } else {
        ureq::tls::TlsProvider::Rustls
    }
}

#[allow(dead_code)]
pub(crate) fn script_http_tls_root_certs_are_expected(root_certs: &ureq::tls::RootCerts) -> bool {
    if is_windows_host() {
        matches!(root_certs, ureq::tls::RootCerts::PlatformVerifier)
    } else {
        matches!(root_certs, ureq::tls::RootCerts::WebPki)
    }
}

pub(crate) fn ipc_default_native_endpoint(
    scope: &crate::platform::contract::ipc::ServerScopeId,
) -> crate::platform::contract::ipc::IpcEndpoint {
    if is_windows_host() {
        crate::platform::contract::ipc::IpcEndpoint::NamedPipe(format!(
            r"\\.\pipe\agenterm-{}",
            scope.as_str()
        ))
    } else {
        crate::platform::contract::ipc::IpcEndpoint::UnixSocket(
            agenterm_platform::ipc::native_runtime_directory()
                .join("agenterm")
                .join(format!("{}.sock", scope.as_str()))
                .to_string_lossy()
                .into_owned(),
        )
    }
}

pub(crate) fn ipc_default_workspace_path(
    scope: &crate::platform::contract::ipc::ServerScopeId,
) -> std::path::PathBuf {
    if is_windows_host() {
        std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(product_directory_name())
            .join("workspaces")
            .join(format!("{}.json", scope.as_str()))
    } else {
        crate::platform::services::ipc::unix_data_root_from(
            std::env::var_os("XDG_DATA_HOME"),
            std::env::var_os("HOME"),
            std::env::temp_dir(),
        )
        .join("workspaces")
        .join(format!("{}.json", scope.as_str()))
    }
}

pub(crate) fn ipc_default_workspace_path_for(
    scope: &crate::platform::contract::ipc::ServerScopeId,
    is_main: bool,
) -> std::path::PathBuf {
    if is_main && is_windows_host() {
        std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(product_directory_name())
            .join("workspace.json")
    } else {
        ipc_default_workspace_path(scope)
    }
}

pub(crate) fn config_root_for_product_directory(
    directories: &agenterm_platform::filesystem::HostDirectories,
) -> std::path::PathBuf {
    directories.config.join(product_directory_name())
}

pub(crate) fn local_data_root_for_product_directory(
    directories: &agenterm_platform::filesystem::HostDirectories,
) -> std::path::PathBuf {
    directories.local_data.join(product_directory_name())
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
        use crate::frontend::toolbar::NativeToolbarHit;
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
            control_center_screenshot_strategy(),
            match agenterm_platform::platform_kind() {
                agenterm_platform::PlatformKind::Windows => {
                    crate::platform::control_center::ScreenshotStrategy::DirectNativeWindow
                }
                agenterm_platform::PlatformKind::Linux | agenterm_platform::PlatformKind::Macos => {
                    crate::platform::control_center::ScreenshotStrategy::RendererRequest
                }
                _ => crate::platform::control_center::ScreenshotStrategy::Unsupported,
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
