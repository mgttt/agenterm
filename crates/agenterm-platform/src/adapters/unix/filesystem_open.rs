use std::{
    ffi::{CString, OsStr},
    fs::File,
    io,
    os::{
        fd::{AsRawFd as _, FromRawFd as _},
        unix::ffi::OsStrExt as _,
    },
    path::Path,
};

use crate::filesystem_open::ExistingEntryType;

pub(crate) fn open_existing(path: &Path, expected: ExistingEntryType) -> io::Result<File> {
    let path = c_string(path.as_os_str())?;
    descriptor(unsafe { libc::open(path.as_ptr(), flags(expected)) })
}

pub(crate) fn open_existing_child(
    parent: &File,
    name: &OsStr,
    expected: ExistingEntryType,
) -> io::Result<File> {
    let name = c_string(name)?;
    descriptor(unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags(expected)) })
}

fn flags(expected: ExistingEntryType) -> libc::c_int {
    libc::O_RDONLY
        | libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | match expected {
            ExistingEntryType::File => 0,
            ExistingEntryType::Directory => libc::O_DIRECTORY,
        }
}

fn c_string(value: &OsStr) -> io::Result<CString> {
    CString::new(value.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))
}

fn descriptor(raw: libc::c_int) -> io::Result<File> {
    if raw < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(raw) })
    }
}
