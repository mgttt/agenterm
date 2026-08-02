//! Linux runtime defaults.

use crate::contract::runtime::TerminalShellDescriptor;

pub fn default_terminal_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
}

pub const fn primary_terminal_shell() -> TerminalShellDescriptor {
    TerminalShellDescriptor {
        id: "sh",
        label: "sh",
        program: "/bin/sh",
    }
}

/// Linux desktop sessions already export locale variables; AgenTerm does not
/// synthesize one here.
pub fn preferred_terminal_lang() -> Option<String> {
    None
}
