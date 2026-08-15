//! Typed preflight information for foreign-window placement.
//!
//! This contract deliberately distinguishes information that is known to be
//! absent from information the host could not establish. Product callers must
//! never interpret an `Unknown` role, operation support, or size constraint as
//! permission to resize an ordinary window.

use std::borrow::Cow;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum PlacementRole {
    Standard,
    Dialog,
    Sheet,
    SystemDialog,
    Other,
    Unknown,
}

impl PlacementRole {
    /// Roles for which product placement may be considered. This is not an
    /// authorization decision; it only rejects roles whose geometry is unsafe
    /// or whose meaning could not be established.
    pub const fn permits_placement(self) -> bool {
        matches!(self, Self::Standard | Self::Dialog)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Support {
    Yes,
    No,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

impl WindowSize {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub const fn is_nonzero(self) -> bool {
        self.width != 0 && self.height != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SizeConstraints {
    /// Constraints reported by a native source. All `None` means the source
    /// positively reported no corresponding limit; it is not `Unknown`.
    Explicit {
        min: Option<WindowSize>,
        max: Option<WindowSize>,
        increment: Option<WindowSize>,
    },
    /// The host exposes no trustworthy numeric preflight limits. The
    /// application/window manager enforces them, so every write requires an
    /// independent bounds readback and callers must report that actual rect.
    ApplicationEnforced,
    /// The adapter could not establish either explicit limits or a reliable
    /// application-enforced/readback contract.
    Unknown,
}

impl SizeConstraints {
    pub fn validate(self) -> Result<(), WindowPlacementError> {
        let Self::Explicit {
            min,
            max,
            increment,
        } = self
        else {
            return Ok(());
        };
        for (name, size) in [("minimum", min), ("maximum", max), ("increment", increment)] {
            if size.is_some_and(|value| !value.is_nonzero()) {
                return Err(WindowPlacementError::failed(
                    "window_constraints_invalid",
                    format!("{name} window size must be nonzero"),
                ));
            }
        }
        if let (Some(min), Some(max)) = (min, max)
            && (min.width > max.width || min.height > max.height)
        {
            return Err(WindowPlacementError::failed(
                "window_constraints_invalid",
                "minimum window size exceeds maximum window size",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlacementWindowInfo {
    pub handle: isize,
    pub process_id: u32,
    pub role: PlacementRole,
    pub movable: Support,
    pub resizable: Support,
    pub constraints: SizeConstraints,
}

impl PlacementWindowInfo {
    pub fn validate(self) -> Result<Self, WindowPlacementError> {
        if self.handle == 0 || self.process_id == 0 {
            return Err(WindowPlacementError::failed(
                "window_identity_invalid",
                "placement inspection returned an empty native identity",
            ));
        }
        self.constraints.validate()?;
        Ok(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum WindowPlacementError {
    Unsupported {
        reason: Cow<'static, str>,
    },
    Failed {
        code: Cow<'static, str>,
        message: String,
    },
}

impl WindowPlacementError {
    pub(crate) fn failed(code: &'static str, message: impl ToString) -> Self {
        Self::Failed {
            code: code.into(),
            message: message.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_standard_and_dialog_roles_permit_placement() {
        assert!(PlacementRole::Standard.permits_placement());
        assert!(PlacementRole::Dialog.permits_placement());
        for role in [
            PlacementRole::Sheet,
            PlacementRole::SystemDialog,
            PlacementRole::Other,
            PlacementRole::Unknown,
        ] {
            assert!(!role.permits_placement(), "role {role:?}");
        }
    }

    #[test]
    fn explicit_absence_is_distinct_from_unknown_constraints() {
        let unbounded = SizeConstraints::Explicit {
            min: None,
            max: None,
            increment: None,
        };
        assert_ne!(unbounded, SizeConstraints::Unknown);
        assert_eq!(unbounded.validate(), Ok(()));
        assert_eq!(SizeConstraints::ApplicationEnforced.validate(), Ok(()));
    }

    #[test]
    fn invalid_explicit_limits_fail_closed() {
        let zero = SizeConstraints::Explicit {
            min: Some(WindowSize::new(0, 10)),
            max: None,
            increment: None,
        };
        assert!(matches!(
            zero.validate(),
            Err(WindowPlacementError::Failed {
                code,
                ..
            }) if code == "window_constraints_invalid"
        ));

        let reversed = SizeConstraints::Explicit {
            min: Some(WindowSize::new(800, 600)),
            max: Some(WindowSize::new(640, 480)),
            increment: None,
        };
        assert!(reversed.validate().is_err());
    }

    #[test]
    fn placement_identity_must_be_stable_and_nonempty() {
        let invalid = PlacementWindowInfo {
            handle: 0,
            process_id: 0,
            role: PlacementRole::Unknown,
            movable: Support::Unknown,
            resizable: Support::Unknown,
            constraints: SizeConstraints::Unknown,
        };
        assert!(matches!(
            invalid.validate(),
            Err(WindowPlacementError::Failed {
                code,
                ..
            }) if code == "window_identity_invalid"
        ));
    }
}
