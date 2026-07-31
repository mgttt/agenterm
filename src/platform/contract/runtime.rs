//! OS-neutral descriptors for native terminal runtime defaults.

/// A shell choice exposed by a platform-neutral terminal UI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TerminalShellDescriptor {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) program: &'static str,
}
