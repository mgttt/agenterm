//! Windows runtime defaults.

use std::{ffi::OsString, io, os::windows::ffi::OsStringExt as _, path::PathBuf, slice};

use crate::{
    contract::runtime::TerminalShellDescriptor, selected::environment::InheritedEnvironment,
};
use windows_sys::Win32::{
    Foundation::{LocalFree, MAX_PATH},
    System::Environment::GetCommandLineW,
    UI::Shell::{CSIDL_APPDATA, CommandLineToArgvW, SHGFP_TYPE_CURRENT, SHGetFolderPathW},
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

/// Resolve roaming application data into caller-owned storage. The legacy
/// shell API is deliberate here: unlike SHGetKnownFolderPath it needs no COM
/// task allocation and therefore has no cross-allocator cleanup edge.
pub fn user_config_directory() -> io::Result<PathBuf> {
    let mut path = [0_u16; MAX_PATH as usize];
    let result = unsafe {
        SHGetFolderPathW(
            std::ptr::null_mut(),
            CSIDL_APPDATA as i32,
            std::ptr::null_mut(),
            SHGFP_TYPE_CURRENT as u32,
            path.as_mut_ptr(),
        )
    };
    if result < 0 {
        return Err(io::Error::other(
            "Windows roaming configuration directory is unavailable",
        ));
    }
    let length = path.iter().position(|unit| *unit == 0).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows returned an unterminated configuration directory",
        )
    })?;
    Ok(PathBuf::from(OsString::from_wide(&path[..length])))
}

pub fn ascii_environment_variable_present(name: &str) -> bool {
    InheritedEnvironment::capture()
        .and_then(|environment| environment.find_ascii(name).map(|value| value.is_some()))
        .unwrap_or(false)
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
    InheritedEnvironment::capture()
        .ok()
        .and_then(|environment| {
            environment
                .find_ascii("COMSPEC")
                .ok()
                .flatten()
                .and_then(|value| String::from_utf16(value).ok())
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| r"C:\Windows\System32\cmd.exe".to_owned())
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
    use super::{
        application_arguments, ascii_environment_variable_present, decode_argument,
        default_terminal_shell, user_config_directory,
    };

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

    #[test]
    fn native_user_config_directory_is_absolute() {
        let path = user_config_directory().expect("native user configuration directory");
        assert!(path.is_absolute());
    }

    #[test]
    fn native_environment_query_matches_process_environment() {
        assert_eq!(
            ascii_environment_variable_present("PATH"),
            std::env::var_os("PATH").is_some()
        );
        assert!(!ascii_environment_variable_present(
            "AGENTERM_ENVIRONMENT_SENTINEL_DOES_NOT_EXIST"
        ));
        assert!(!default_terminal_shell().is_empty());
    }
}
