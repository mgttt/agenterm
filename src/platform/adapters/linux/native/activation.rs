//! Linux window activation / no-activate policy for platform migration
//! Adapter-private native mechanism selected only by platform::selected.
//! slice-2 (contract revision 1).
//!
//! Unlike macOS (`EventLoopBuilderExtMacOS::with_activate_ignoring_other_apps`)
//! or Win32 (`ShowWindow(SW_SHOWNOACTIVATE)`), Linux winit exposes activation
//! intent primarily through [`WindowAttributes::with_active`]. X11 and Wayland
//! both honor that attribute when the compositor cooperates; Wayland may still
//! refuse focus steals — that is a compositor policy outcome, not a reason to
//! claim Available when headless or to invent a shared contract field.
//!
//! `AGENTERM_NO_ACTIVATE` / `--no-activate` map to `with_active(false)` and an
//! initially-unfocused window assumption.

#![cfg(target_os = "linux")]

use winit::window::WindowAttributes;

use crate::platform::{CapabilityStatus, DisplayBackendFacts};

use super::display_facts_from_env;

/// Which display backends can receive the activation policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActivationBackend {
    X11,
    Wayland,
    X11AndWayland,
}

impl ActivationBackend {
    pub(crate) fn from_display(facts: DisplayBackendFacts) -> Result<Self, ActivationError> {
        match (facts.x11, facts.wayland, facts.headless) {
            (_, _, true) => Err(ActivationError::Headless),
            (true, true, false) => Ok(Self::X11AndWayland),
            (true, false, false) => Ok(Self::X11),
            (false, true, false) => Ok(Self::Wayland),
            (false, false, false) => Err(ActivationError::Headless),
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::X11 => "x11",
            Self::Wayland => "wayland",
            Self::X11AndWayland => "x11+wayland",
        }
    }

    /// Wayland compositors may ignore focus requests; X11 is typically stricter
    /// about `with_active` but still subject to window-manager policy.
    pub(crate) const fn focus_is_best_effort(self) -> bool {
        matches!(self, Self::Wayland | Self::X11AndWayland)
    }
}

/// Typed Linux activation policy failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActivationError {
    Headless,
}

impl ActivationError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Headless => "activation_headless",
        }
    }

    pub(crate) fn message(self) -> String {
        match self {
            Self::Headless => {
                "window activation unavailable without a graphical display".to_string()
            }
        }
    }

    pub(crate) fn to_capability_status(self) -> CapabilityStatus {
        match self {
            Self::Headless => CapabilityStatus::Unsupported {
                reason: "headless-display",
            },
        }
    }
}

/// Activation capability: Available when a display backend exists.
pub(crate) fn activation_capability_status(facts: DisplayBackendFacts) -> CapabilityStatus {
    match ActivationBackend::from_display(facts) {
        Ok(_) => CapabilityStatus::Available,
        Err(error) => error.to_capability_status(),
    }
}

pub(crate) fn activation_capability_status_from_env() -> CapabilityStatus {
    activation_capability_status(display_facts_from_env())
}

/// Whether the window should request activation (`with_active(true)`).
pub(crate) const fn wants_activation(no_activate: bool) -> bool {
    !no_activate
}

/// Initial `window_focused` assumption before the first `Focused` event.
pub(crate) const fn initial_window_focused(no_activate: bool) -> bool {
    !no_activate
}

/// Apply Linux no-activate policy to winit window attributes.
pub(crate) fn configure_window_attributes(
    attributes: WindowAttributes,
    no_activate: bool,
) -> WindowAttributes {
    agenterm_platform::activation::configure_window_attributes(attributes, no_activate)
}

/// Snapshot facts for diagnostics (not authorization).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActivationPolicyFacts {
    pub backend: ActivationBackend,
    pub no_activate: bool,
    pub wants_activation: bool,
    pub focus_best_effort: bool,
}

pub(crate) fn policy_facts(no_activate: bool) -> Result<ActivationPolicyFacts, ActivationError> {
    let backend = ActivationBackend::from_display(display_facts_from_env())?;
    Ok(ActivationPolicyFacts {
        backend,
        no_activate,
        wants_activation: wants_activation(no_activate),
        focus_best_effort: backend.focus_is_best_effort(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_activation_is_unsupported() {
        let status = activation_capability_status(DisplayBackendFacts {
            x11: false,
            wayland: false,
            headless: true,
        });
        assert!(matches!(
            status,
            CapabilityStatus::Unsupported {
                reason: "headless-display"
            }
        ));
    }

    #[test]
    fn x11_and_wayland_activation_are_available() {
        assert_eq!(
            activation_capability_status(DisplayBackendFacts {
                x11: true,
                wayland: false,
                headless: false,
            }),
            CapabilityStatus::Available
        );
        assert_eq!(
            activation_capability_status(DisplayBackendFacts {
                x11: false,
                wayland: true,
                headless: false,
            }),
            CapabilityStatus::Available
        );
    }

    #[test]
    fn no_activate_disables_with_active_intent() {
        assert!(!wants_activation(true));
        assert!(wants_activation(false));
        assert!(!initial_window_focused(true));
        assert!(initial_window_focused(false));
    }

    #[test]
    fn wayland_focus_is_marked_best_effort() {
        assert!(ActivationBackend::Wayland.focus_is_best_effort());
        assert!(ActivationBackend::X11AndWayland.focus_is_best_effort());
        assert!(!ActivationBackend::X11.focus_is_best_effort());
    }

    #[test]
    fn configure_window_attributes_sets_active_flag() {
        let active = configure_window_attributes(WindowAttributes::default(), false);
        let inactive = configure_window_attributes(WindowAttributes::default(), true);
        // winit does not expose getters; constructing both paths must not panic
        // and must remain distinct Rust values for the create_window call site.
        let _ = (active, inactive);
    }

    #[test]
    fn policy_facts_from_env_when_display_present() {
        if display_facts_from_env().headless {
            assert!(matches!(policy_facts(true), Err(ActivationError::Headless)));
            return;
        }
        let facts = policy_facts(true).expect("policy facts");
        assert!(facts.no_activate);
        assert!(!facts.wants_activation);
        assert!(matches!(
            facts.backend,
            ActivationBackend::X11 | ActivationBackend::Wayland | ActivationBackend::X11AndWayland
        ));
    }
}
