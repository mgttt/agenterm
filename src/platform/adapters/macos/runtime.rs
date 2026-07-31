//! macOS runtime defaults.

use crate::platform::contract::runtime::TerminalShellDescriptor;

pub(crate) fn default_terminal_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
}

pub(crate) const fn primary_terminal_shell() -> TerminalShellDescriptor {
    TerminalShellDescriptor {
        id: "zsh",
        label: "zsh",
        program: "/bin/zsh",
    }
}
