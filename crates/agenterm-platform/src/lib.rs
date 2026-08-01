//! Reusable operating-system contracts and capability facades.
//!
//! Product policy stays in the embedding crate. This crate exposes only
//! platform-neutral types and selected native mechanisms.

use std::borrow::Cow;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum PlatformKind {
    Windows,
    Macos,
    Linux,
}

pub const fn platform_kind() -> PlatformKind {
    selected::platform_kind()
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Capability {
    Process,
    Filesystem,
    Locking,
    Ipc,
    Pty,
    Window,
    Input,
    Ime,
    Activation,
    Clipboard,
    Screenshot,
    Font,
    WebView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CapabilityStatus {
    Available,
    Unsupported {
        reason: Cow<'static, str>,
    },
    Failed {
        code: Cow<'static, str>,
        message: String,
    },
}

pub fn capability_status(capability: Capability) -> CapabilityStatus {
    let enabled = match capability {
        Capability::Process => cfg!(feature = "process"),
        Capability::Filesystem => cfg!(feature = "filesystem"),
        Capability::Locking => cfg!(feature = "locking"),
        Capability::Ipc => cfg!(feature = "ipc"),
        Capability::Pty => cfg!(feature = "pty"),
        Capability::Window => cfg!(feature = "window"),
        Capability::Input => cfg!(feature = "input"),
        Capability::Ime => cfg!(feature = "ime"),
        Capability::Activation => cfg!(feature = "activation"),
        Capability::Clipboard => cfg!(feature = "clipboard"),
        Capability::Screenshot => cfg!(feature = "screenshot"),
        Capability::Font => cfg!(feature = "font"),
        Capability::WebView => cfg!(feature = "webview"),
    };
    if enabled {
        CapabilityStatus::Available
    } else {
        CapabilityStatus::Unsupported {
            reason: Cow::Borrowed("feature-disabled"),
        }
    }
}

pub mod contract;

#[cfg(feature = "pty")]
pub mod pty;

#[cfg(feature = "process")]
pub mod process;

#[cfg(feature = "process")]
pub mod runtime;

mod selected;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_capabilities_are_explicit() {
        #[cfg(not(feature = "ipc"))]
        assert_eq!(
            capability_status(Capability::Ipc),
            CapabilityStatus::Unsupported {
                reason: Cow::Borrowed("feature-disabled")
            }
        );
    }
}
