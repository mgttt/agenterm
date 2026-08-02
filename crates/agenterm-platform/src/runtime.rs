//! OS-neutral runtime-default service.

use crate::{contract::runtime::TerminalShellDescriptor, selected};

pub fn default_terminal_shell() -> String {
    selected::runtime::default_terminal_shell()
}

pub fn primary_terminal_shell() -> TerminalShellDescriptor {
    selected::runtime::primary_terminal_shell()
}

pub fn preferred_terminal_lang() -> Option<String> {
    selected::runtime::preferred_terminal_lang()
}
