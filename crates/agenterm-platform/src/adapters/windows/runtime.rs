//! Windows runtime defaults.

use crate::contract::runtime::TerminalShellDescriptor;

pub fn default_terminal_shell() -> String {
    std::env::var("COMSPEC").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".to_owned())
}

pub const fn primary_terminal_shell() -> TerminalShellDescriptor {
    TerminalShellDescriptor {
        id: "cmd",
        label: "cmd",
        program: r"C:\Windows\System32\cmd.exe",
    }
}
