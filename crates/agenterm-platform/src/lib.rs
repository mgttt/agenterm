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
    let (enabled, implemented) = match capability {
        Capability::Process => (cfg!(feature = "process"), true),
        Capability::Filesystem => (cfg!(feature = "filesystem"), true),
        Capability::Locking => (cfg!(feature = "locking"), true),
        Capability::Ipc => (cfg!(feature = "ipc"), true),
        Capability::Pty => (cfg!(feature = "pty"), true),
        Capability::Window => (cfg!(feature = "window"), true),
        Capability::Input => (cfg!(feature = "input"), true),
        Capability::Ime => (cfg!(feature = "ime"), true),
        Capability::Activation => (cfg!(feature = "activation"), true),
        Capability::Clipboard => (cfg!(feature = "clipboard"), true),
        Capability::Screenshot => (cfg!(feature = "screenshot"), true),
        Capability::Font => (cfg!(feature = "font"), true),
        Capability::WebView => (cfg!(feature = "webview"), false),
    };
    if enabled && implemented {
        CapabilityStatus::Available
    } else if enabled {
        CapabilityStatus::Unsupported {
            reason: Cow::Borrowed("capability-not-yet-implemented"),
        }
    } else {
        CapabilityStatus::Unsupported {
            reason: Cow::Borrowed("feature-disabled"),
        }
    }
}

pub mod contract;

#[cfg(feature = "activation")]
pub mod activation;

#[cfg(feature = "clipboard")]
pub mod clipboard;

#[cfg(feature = "font")]
pub mod font;

#[cfg(feature = "filesystem")]
pub mod filesystem;

#[cfg(feature = "locking")]
pub mod locking;

#[cfg(feature = "ipc")]
pub mod ipc;

#[cfg(feature = "input")]
pub mod input;

#[cfg(feature = "ime")]
pub mod ime;

#[cfg(feature = "window")]
pub mod window;

#[cfg(feature = "pty")]
pub mod pty;

#[cfg(feature = "process")]
pub mod process;

#[cfg(feature = "process")]
pub mod runtime;

#[cfg(feature = "screenshot")]
pub mod screenshot;

mod selected;

#[cfg(test)]
mod tests {
    #[test]
    fn disabled_capabilities_are_explicit() {
        #[cfg(not(feature = "ipc"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Ipc),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }

    #[test]
    fn declared_but_unimplemented_capabilities_are_explicit() {
        #[cfg(feature = "webview")]
        assert_eq!(
            crate::capability_status(crate::Capability::WebView),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("capability-not-yet-implemented")
            }
        );
    }
}
