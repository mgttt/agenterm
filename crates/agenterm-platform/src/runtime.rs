//! OS-neutral runtime-default service.

use std::{io, path::PathBuf};

use crate::{contract::runtime::TerminalShellDescriptor, selected};

/// Return this process's UTF-8 application arguments, excluding the image
/// name. Invalid native text is reported instead of panicking in a GUI entry.
pub fn application_arguments() -> io::Result<Vec<String>> {
    selected::runtime::application_arguments()
}

/// Return the host's per-user roaming configuration directory.
pub fn user_config_directory() -> io::Result<PathBuf> {
    selected::runtime::user_config_directory()
}

pub fn default_terminal_shell() -> String {
    selected::runtime::default_terminal_shell()
}

pub fn primary_terminal_shell() -> TerminalShellDescriptor {
    selected::runtime::primary_terminal_shell()
}

pub fn preferred_terminal_lang() -> Option<String> {
    selected::runtime::preferred_terminal_lang()
}
