//! Windows runtime defaults.

use std::{io, slice};

use crate::contract::runtime::TerminalShellDescriptor;
use windows_sys::Win32::{
    Foundation::LocalFree, System::Environment::GetCommandLineW, UI::Shell::CommandLineToArgvW,
};

const MAX_COMMAND_LINE_UNITS: usize = 32_768;

struct LocalArgv(*mut *mut u16);

impl Drop for LocalArgv {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = LocalFree(self.0.cast());
            }
        }
    }
}

/// Parse the command line with the Windows-owned parser and release its
/// LocalAlloc buffer on every return path. The image name occupies argv[0]
/// and is intentionally omitted from the product-facing result.
pub fn application_arguments() -> io::Result<Vec<String>> {
    let mut count = 0_i32;
    let argv = unsafe { CommandLineToArgvW(GetCommandLineW(), &raw mut count) };
    if argv.is_null() {
        return Err(io::Error::last_os_error());
    }
    let argv = LocalArgv(argv);
    let count = usize::try_from(count)
        .ok()
        .filter(|count| *count <= MAX_COMMAND_LINE_UNITS)
        .ok_or_else(invalid_native_arguments)?;
    let pointers = unsafe { slice::from_raw_parts(argv.0, count) };
    pointers
        .iter()
        .skip(1)
        .map(|argument| decode_argument(*argument))
        .collect()
}

fn decode_argument(argument: *const u16) -> io::Result<String> {
    if argument.is_null() {
        return Err(invalid_native_arguments());
    }
    let mut length = 0;
    while length < MAX_COMMAND_LINE_UNITS {
        if unsafe { *argument.add(length) } == 0 {
            let units = unsafe { slice::from_raw_parts(argument, length) };
            return String::from_utf16(units).map_err(|_| invalid_native_arguments());
        }
        length += 1;
    }
    Err(invalid_native_arguments())
}

fn invalid_native_arguments() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Windows supplied invalid process arguments",
    )
}

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

/// Windows terminals do not use POSIX locale variables.
pub fn preferred_terminal_lang() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::{application_arguments, decode_argument};

    #[test]
    fn native_arguments_match_rust_for_the_test_process() {
        assert_eq!(
            application_arguments().expect("native process arguments"),
            std::env::args().skip(1).collect::<Vec<_>>()
        );
    }

    #[test]
    fn invalid_utf16_is_a_typed_failure() {
        let argument = [0xd800, 0];
        let error = decode_argument(argument.as_ptr()).expect_err("unpaired surrogate");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }
}
