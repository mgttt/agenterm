//! Unix placeholder: input injection is not yet wired on Linux/macOS.

use crate::CapabilityStatus;
use crate::contract::input_inject::{InputInjectError, PointerButton, PointerPosition};

pub(crate) fn capability_status() -> CapabilityStatus {
    CapabilityStatus::Unsupported {
        reason: "input-inject not wired on unix".into(),
    }
}

pub(crate) fn pointer_move(_position: PointerPosition) -> Result<(), InputInjectError> {
    Err(InputInjectError::Unsupported {
        reason: "input-inject not wired on unix".into(),
    })
}

pub(crate) fn pointer_position() -> Result<PointerPosition, InputInjectError> {
    Err(InputInjectError::Unsupported {
        reason: "pointer-position not wired on unix".into(),
    })
}

pub(crate) fn pointer_click(
    _position: PointerPosition,
    _button: PointerButton,
    _clicks: u32,
) -> Result<(), InputInjectError> {
    Err(InputInjectError::Unsupported {
        reason: "input-inject not wired on unix".into(),
    })
}

pub(crate) fn type_text(_text: &str) -> Result<(), InputInjectError> {
    Err(InputInjectError::Unsupported {
        reason: "input-inject not wired on unix".into(),
    })
}

pub(crate) fn send_keys(_shortcut: &str) -> Result<(), InputInjectError> {
    Err(InputInjectError::Unsupported {
        reason: "input-inject not wired on unix".into(),
    })
}
