//! Linux runtime defaults.

use std::io;

use crate::contract::runtime::TerminalShellDescriptor;

pub fn application_arguments() -> io::Result<Vec<String>> {
    std::env::args_os()
        .skip(1)
        .map(|argument| {
            argument.into_string().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Linux supplied invalid UTF-8 arguments",
                )
            })
        })
        .collect()
}

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
