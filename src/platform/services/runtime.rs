//! OS-neutral runtime-default service.

use crate::platform::{contract::runtime::TerminalShellDescriptor, selected};

pub(crate) fn default_terminal_shell() -> String {
    selected::runtime::default_terminal_shell()
}

pub(crate) fn primary_terminal_shell() -> TerminalShellDescriptor {
    selected::runtime::primary_terminal_shell()
}
