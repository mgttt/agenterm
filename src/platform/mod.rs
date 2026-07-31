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

#[cfg(any(target_os = "linux", target_os = "macos", test))]
pub(crate) mod scale;
pub(crate) mod window;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

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

pub(crate) fn platform_info_json() -> serde_json::Value {
    #[cfg(target_os = "windows")]
    let (kind, status): (PlatformKind, fn(CapabilityKind) -> CapabilityStatus) =
        (windows::platform_kind(), windows::capability_status);
    #[cfg(target_os = "linux")]
    let (kind, status): (PlatformKind, fn(CapabilityKind) -> CapabilityStatus) =
        (linux::platform_kind(), linux::capability_status);
    #[cfg(target_os = "macos")]
    let (kind, status): (PlatformKind, fn(CapabilityKind) -> CapabilityStatus) =
        (macos::platform_kind(), macos::capability_status);

    let capabilities = CapabilityKind::ALL
        .into_iter()
        .map(|capability| (capability.as_str().to_owned(), status(capability).to_json()))
        .collect::<serde_json::Map<_, _>>();
    serde_json::json!({
        "contract_revision": CONTRACT_REVISION,
        "kind": match kind {
            PlatformKind::Windows => "windows",
            PlatformKind::Macos => "macos",
            PlatformKind::Linux => "linux",
        },
        "capabilities": capabilities,
    })
}

/// Modifier bits carried on normalized key events.
///
/// Which modifier is the platform *primary* shortcut chord remains an adapter
/// decision (Control vs Super/Command); shared helpers only expose the bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModifierState {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
    /// Super on Linux, Win on Windows, Command on macOS.
    pub meta: bool,
}

impl ModifierState {
    pub const fn empty() -> Self {
        Self {
            control: false,
            shift: false,
            alt: false,
            meta: false,
        }
    }

    /// Control or meta held — common primary-chord probe used by slice-1
    /// adapters. Platform-specific primary policy still belongs in the adapter.
    #[allow(dead_code)]
    pub const fn control_or_meta(self) -> bool {
        self.control || self.meta
    }
}

/// Classification of one key press before product surfaces consume it.
///
/// Text commits must stay distinct from shortcut chords so Shift punctuation,
/// layouts, dead keys, CJK, and terminal control keys keep native meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyClassification {
    Shortcut {
        key: String,
        modifiers: ModifierState,
    },
    TextCommit(String),
    ControlKey {
        name: String,
        modifiers: ModifierState,
    },
    Ignored,
}

/// Display / window-system discovery facts (not auth).
///
/// Linux populates X11/Wayland; other platforms leave those flags false and
/// report headless through their own window capability diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub struct DisplayBackendFacts {
    pub x11: bool,
    pub wayland: bool,
    pub headless: bool,
}

/// Separate shortcut chords from committed Unicode text.
///
/// `is_shortcut` is decided by the native adapter because Control, Command,
/// Super, and Windows AltGr layouts do not share one primary-chord policy.
/// `committed_text` is whatever the native path already resolved. When present
/// and `is_shortcut` is false, prefer [`KeyClassification::TextCommit`] over
/// physical-key synthesis.
pub fn classify_key_press(
    is_shortcut: bool,
    modifiers: ModifierState,
    logical_character: Option<&str>,
    named_key: Option<&str>,
    committed_text: Option<&str>,
) -> KeyClassification {
    if is_shortcut {
        if let Some(ch) = logical_character {
            return KeyClassification::Shortcut {
                key: ch.to_string(),
                modifiers,
            };
        }
        if let Some(name) = named_key {
            return KeyClassification::Shortcut {
                key: name.to_string(),
                modifiers,
            };
        }
        return KeyClassification::Ignored;
    }

    if let Some(text) = committed_text.filter(|value| !value.is_empty()) {
        return KeyClassification::TextCommit(text.to_string());
    }

    if let Some(name) = named_key {
        return KeyClassification::ControlKey {
            name: name.to_string(),
            modifiers,
        };
    }

    if let Some(ch) = logical_character.filter(|value| !value.is_empty()) {
        return KeyClassification::TextCommit(ch.to_string());
    }

    KeyClassification::Ignored
}

/// Inclusive screenshot clip rectangle in framebuffer pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenshotClipRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Cross-platform clip validation failures (stable semantics for all adapters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenshotClipError {
    EmptyFrame,
    ZeroDimension,
    OriginOutside,
    Overflow,
}

/// Reject empty frames, zero clips, and overflow without silent shrink.
pub fn validate_screenshot_clip(
    frame_width: u32,
    frame_height: u32,
    clip: ScreenshotClipRect,
) -> Result<(), ScreenshotClipError> {
    if frame_width == 0 || frame_height == 0 {
        return Err(ScreenshotClipError::EmptyFrame);
    }
    if clip.width == 0 || clip.height == 0 {
        return Err(ScreenshotClipError::ZeroDimension);
    }
    if clip.x >= frame_width || clip.y >= frame_height {
        return Err(ScreenshotClipError::OriginOutside);
    }
    let right = clip
        .x
        .checked_add(clip.width)
        .ok_or(ScreenshotClipError::Overflow)?;
    let bottom = clip
        .y
        .checked_add(clip.height)
        .ok_or(ScreenshotClipError::Overflow)?;
    if right > frame_width || bottom > frame_height {
        return Err(ScreenshotClipError::Overflow);
    }
    Ok(())
}

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

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_toolbar_order_matches_contract() {
        use crate::platform::linux::toolbar::LinuxToolbarHit;
        assert_eq!(
            LinuxToolbarHit::ORDER.map(LinuxToolbarHit::action_id),
            action::TOOLBAR_ACTION_ORDER
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_toolbar_order_matches_contract() {
        use crate::platform::macos::toolbar::MacosToolbarHit;
        assert_eq!(
            MacosToolbarHit::ORDER.map(MacosToolbarHit::action_id),
            action::TOOLBAR_ACTION_ORDER
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_toolbar_order_matches_contract() {
        use crate::platform::windows::toolbar::WindowsToolbarHit;
        assert_eq!(
            WindowsToolbarHit::ORDER.map(WindowsToolbarHit::action_id),
            action::TOOLBAR_ACTION_ORDER
        );
    }

    #[test]
    fn screenshot_clip_validation_rejects_overflow_without_shrink() {
        let frame = (950_u32, 594_u32);
        assert!(
            validate_screenshot_clip(
                frame.0,
                frame.1,
                ScreenshotClipRect {
                    x: 0,
                    y: 0,
                    width: frame.0,
                    height: frame.1,
                },
            )
            .is_ok()
        );
        assert_eq!(
            validate_screenshot_clip(
                frame.0,
                frame.1,
                ScreenshotClipRect {
                    x: 200,
                    y: 100,
                    width: 800,
                    height: 500,
                },
            ),
            Err(ScreenshotClipError::Overflow)
        );
        assert_eq!(
            validate_screenshot_clip(
                0,
                10,
                ScreenshotClipRect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                }
            ),
            Err(ScreenshotClipError::EmptyFrame)
        );
        assert_eq!(
            validate_screenshot_clip(
                10,
                10,
                ScreenshotClipRect {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 5,
                }
            ),
            Err(ScreenshotClipError::ZeroDimension)
        );
        assert_eq!(
            validate_screenshot_clip(
                10,
                10,
                ScreenshotClipRect {
                    x: 10,
                    y: 0,
                    width: 1,
                    height: 1,
                }
            ),
            Err(ScreenshotClipError::OriginOutside)
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
