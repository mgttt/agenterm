//! Linux runtime defaults.

use std::{io, path::PathBuf};

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

pub fn user_config_directory() -> io::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".config"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is unavailable"))
}

pub fn ascii_environment_variable_present(name: &str) -> bool {
    name.is_ascii() && std::env::var_os(name).is_some()
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
