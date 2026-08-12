//! Borrowed access to the process environment block owned by Windows.

use std::{io, slice};

#[cfg(target_arch = "x86_64")]
use core::arch::asm;

use windows_sys::Win32::System::Environment::{FreeEnvironmentStringsW, GetEnvironmentStringsW};

const MAX_ENVIRONMENT_UNITS: usize = 32 * 1024 * 1024;

/// RAII owner for the UTF-16 block returned by `GetEnvironmentStringsW`.
pub(crate) struct InheritedEnvironment(*mut u16);

impl InheritedEnvironment {
    pub(crate) fn capture() -> io::Result<Self> {
        let block = unsafe { GetEnvironmentStringsW() };
        if block.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(block))
        }
    }

    pub(crate) fn units(&self) -> io::Result<&[u16]> {
        let mut length = 0usize;
        while length < MAX_ENVIRONMENT_UNITS {
            let unit = unsafe { *self.0.add(length) };
            length += 1;
            if unit == 0 && (length == 1 || unsafe { *self.0.add(length) } == 0) {
                if length != 1 {
                    length += 1;
                }
                return Ok(unsafe { slice::from_raw_parts(self.0, length) });
            }
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "inherited environment block is not terminated",
        ))
    }

    /// Find a fixed ASCII key with Windows' case-insensitive environment-key
    /// semantics. The returned UTF-16 value borrows this captured block.
    pub(crate) fn find_ascii(&self, name: &str) -> io::Result<Option<&[u16]>> {
        if name.is_empty() || !name.is_ascii() || name.as_bytes().contains(&b'=') {
            return Ok(None);
        }

        #[cfg(target_arch = "x86_64")]
        {
            let mut value_length = 0usize;
            let value = unsafe {
                find_ascii_x64(
                    self.0,
                    MAX_ENVIRONMENT_UNITS,
                    name.as_ptr(),
                    name.len(),
                    &raw mut value_length,
                )
            };
            if value.is_null() {
                return if value_length == usize::MAX {
                    Err(invalid_environment_block())
                } else {
                    Ok(None)
                };
            }
            Ok(Some(unsafe { slice::from_raw_parts(value, value_length) }))
        }

        #[cfg(not(target_arch = "x86_64"))]
        self.find_ascii_portable(name)
    }

    #[cfg(not(target_arch = "x86_64"))]
    fn find_ascii_portable(&self, name: &str) -> io::Result<Option<&[u16]>> {
        let units = self.units()?;
        let mut offset = 0usize;
        while offset < units.len() {
            let tail = &units[offset..];
            let Some(length) = tail.iter().position(|unit| *unit == 0) else {
                break;
            };
            if length == 0 {
                break;
            }
            let entry = &tail[..length];
            if let Some(separator) = entry.iter().position(|unit| *unit == b'=' as u16)
                && ascii_name_matches(&entry[..separator], name.as_bytes())
            {
                return Ok(Some(&entry[separator + 1..]));
            }
            offset += length + 1;
        }
        Ok(None)
    }
}

impl Drop for InheritedEnvironment {
    fn drop(&mut self) {
        unsafe {
            FreeEnvironmentStringsW(self.0);
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn ascii_name_matches(wide: &[u16], ascii: &[u8]) -> bool {
    wide.len() == ascii.len()
        && wide
            .iter()
            .zip(ascii)
            .all(|(left, right)| ascii_upper_unit(*left) == right.to_ascii_uppercase() as u16)
}

#[cfg(not(target_arch = "x86_64"))]
const fn ascii_upper_unit(unit: u16) -> u16 {
    if unit >= b'a' as u16 && unit <= b'z' as u16 {
        unit - (b'a' - b'A') as u16
    } else {
        unit
    }
}

#[cfg(target_arch = "x86_64")]
fn invalid_environment_block() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "inherited environment block is not terminated",
    )
}

/// Search a Windows UTF-16 environment block without allocation. A null
/// result with `value_length == usize::MAX` reports a missing terminator;
/// another null result means the key is absent.
#[cfg(target_arch = "x86_64")]
unsafe fn find_ascii_x64(
    cursor: *const u16,
    maximum_units: usize,
    name: *const u8,
    name_length: usize,
    value_length: *mut usize,
) -> *const u16 {
    let remaining = maximum_units;
    let result: *const u16;
    let _entry_length: usize;
    let _index: usize;
    let _wide: usize;
    let _narrow: usize;
    unsafe {
        asm!(
            "2:",
            "test {remaining}, {remaining}",
            "jz 9f",
            "cmp word ptr [{cursor}], 0",
            "je 8f",
            "xor {entry_length}, {entry_length}",
            "3:",
            "cmp {entry_length}, {remaining}",
            "jae 9f",
            "cmp word ptr [{cursor} + {entry_length}*2], 0",
            "je 4f",
            "inc {entry_length}",
            "jmp 3b",
            "4:",
            "cmp {entry_length}, {name_length}",
            "jbe 7f",
            "cmp word ptr [{cursor} + {name_length}*2], 61",
            "jne 7f",
            "xor {index}, {index}",
            "5:",
            "cmp {index}, {name_length}",
            "je 6f",
            "movzx {wide}, word ptr [{cursor} + {index}*2]",
            "cmp {wide}, 127",
            "ja 7f",
            "cmp {wide}, 65",
            "jb 3f",
            "cmp {wide}, 90",
            "ja 3f",
            "add {wide}, 32",
            "3:",
            "movzx {narrow}, byte ptr [{name} + {index}]",
            "cmp {narrow}, 65",
            "jb 4f",
            "cmp {narrow}, 90",
            "ja 4f",
            "add {narrow}, 32",
            "4:",
            "cmp {wide}, {narrow}",
            "jne 7f",
            "inc {index}",
            "jmp 5b",
            "6:",
            "lea {result}, [{cursor} + {name_length}*2 + 2]",
            "sub {entry_length}, {name_length}",
            "dec {entry_length}",
            "mov [{value_length}], {entry_length}",
            "jmp 5f",
            "7:",
            "lea {cursor}, [{cursor} + {entry_length}*2 + 2]",
            "inc {entry_length}",
            "sub {remaining}, {entry_length}",
            "jmp 2b",
            "8:",
            "xor {result}, {result}",
            "mov qword ptr [{value_length}], 0",
            "jmp 5f",
            "9:",
            "xor {result}, {result}",
            "mov qword ptr [{value_length}], -1",
            "5:",
            cursor = inout(reg) cursor => _,
            remaining = inout(reg) remaining => _,
            name = in(reg) name,
            name_length = in(reg) name_length,
            value_length = in(reg) value_length,
            result = out(reg) result,
            entry_length = out(reg) _entry_length,
            index = out(reg) _index,
            wide = out(reg) _wide,
            narrow = out(reg) _narrow,
            options(nostack)
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::InheritedEnvironment;

    #[cfg(target_arch = "x86_64")]
    use super::find_ascii_x64;

    #[test]
    fn native_block_finds_ascii_names_case_insensitively() {
        let environment = InheritedEnvironment::capture().expect("process environment");
        assert!(
            environment
                .find_ascii("path")
                .expect("valid block")
                .is_some()
        );
        assert!(
            environment
                .find_ascii("AGENTERM_ENVIRONMENT_SENTINEL_DOES_NOT_EXIST")
                .expect("valid block")
                .is_none()
        );
        assert!(
            environment
                .find_ascii("环境")
                .expect("invalid key")
                .is_none()
        );
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn x64_scanner_covers_values_drive_entries_and_truncation() {
        let mut block = Vec::new();
        for entry in ["=C:=C:\\work", "Path=C:\\bin", "EMPTY=", "Mixed=Value"] {
            block.extend(entry.encode_utf16());
            block.push(0);
        }
        block.push(0);

        assert_eq!(scan(&block, "path"), Ok(Some("C:\\bin".to_owned())));
        assert_eq!(scan(&block, "EMPTY"), Ok(Some(String::new())));
        assert_eq!(scan(&block, "mIxEd"), Ok(Some("Value".to_owned())));
        assert_eq!(scan(&block, "C"), Ok(None));
        assert_eq!(scan(&block, "missing"), Ok(None));

        let truncated = "PATH=value\0".encode_utf16().collect::<Vec<_>>();
        assert_eq!(scan(&truncated, "missing"), Err(()));
    }

    #[cfg(target_arch = "x86_64")]
    fn scan(block: &[u16], name: &str) -> Result<Option<String>, ()> {
        let mut length = 0usize;
        let value = unsafe {
            find_ascii_x64(
                block.as_ptr(),
                block.len(),
                name.as_ptr(),
                name.len(),
                &raw mut length,
            )
        };
        if value.is_null() {
            return if length == usize::MAX {
                Err(())
            } else {
                Ok(None)
            };
        }
        let value = unsafe { std::slice::from_raw_parts(value, length) };
        String::from_utf16(value).map(Some).map_err(|_| ())
    }
}
