use std::{
    ffi::OsStr,
    fs::{File, OpenOptions},
    io,
    os::windows::{
        ffi::OsStrExt as _,
        fs::OpenOptionsExt as _,
        io::{AsRawHandle as _, FromRawHandle as _},
    },
    path::Path,
};

use windows_sys::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    },
};

use crate::filesystem_open::{ExistingEntryAccess, ExistingEntryType};

const FILE_LIST_DIRECTORY: u32 = 0x0001;
const FILE_READ_DATA: u32 = 0x0001;
const FILE_READ_ATTRIBUTES: u32 = 0x0080;
const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;
const OBJ_CASE_INSENSITIVE: u32 = 0x0040;
const FILE_OPEN: u32 = 1;
const FILE_DIRECTORY_FILE: u32 = 0x0001;
const FILE_NON_DIRECTORY_FILE: u32 = 0x0040;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x0020;
const FILE_OPEN_REPARSE_POINT_OPTION: u32 = 0x0020_0000;

#[repr(C)]
struct NtUnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

#[repr(C)]
struct NtObjectAttributes {
    length: u32,
    root_directory: HANDLE,
    object_name: *mut NtUnicodeString,
    attributes: u32,
    security_descriptor: *mut core::ffi::c_void,
    security_quality_of_service: *mut core::ffi::c_void,
}

#[repr(C)]
struct NtIoStatusBlock {
    status: isize,
    information: usize,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        file_handle: *mut HANDLE,
        desired_access: u32,
        object_attributes: *mut NtObjectAttributes,
        io_status_block: *mut NtIoStatusBlock,
        allocation_size: *mut i64,
        file_attributes: u32,
        share_access: u32,
        create_disposition: u32,
        create_options: u32,
        ea_buffer: *mut core::ffi::c_void,
        ea_length: u32,
    ) -> i32;
}

pub(crate) fn open_existing(
    path: &Path,
    expected: ExistingEntryType,
    access: ExistingEntryAccess,
) -> io::Result<File> {
    reject_nul(path.as_os_str())?;
    OpenOptions::new()
        .access_mode(desired_access(expected, access))
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

pub(crate) fn open_existing_child(
    parent: &File,
    name: &OsStr,
    expected: ExistingEntryType,
    access: ExistingEntryAccess,
) -> io::Result<File> {
    let mut name: Vec<u16> = name.encode_wide().collect();
    if name.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains NUL",
        ));
    }
    let byte_length = name
        .len()
        .checked_mul(2)
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "child name is too long"))?;
    let mut unicode = NtUnicodeString {
        length: byte_length,
        maximum_length: byte_length,
        buffer: name.as_mut_ptr(),
    };
    let mut attributes = NtObjectAttributes {
        length: std::mem::size_of::<NtObjectAttributes>() as u32,
        root_directory: parent.as_raw_handle(),
        object_name: &raw mut unicode,
        attributes: OBJ_CASE_INSENSITIVE,
        security_descriptor: std::ptr::null_mut(),
        security_quality_of_service: std::ptr::null_mut(),
    };
    let mut io_status = NtIoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut opened = std::ptr::null_mut();
    let options = FILE_SYNCHRONOUS_IO_NONALERT
        | FILE_OPEN_REPARSE_POINT_OPTION
        | match expected {
            ExistingEntryType::File => FILE_NON_DIRECTORY_FILE,
            ExistingEntryType::Directory => FILE_DIRECTORY_FILE,
        };
    let status = unsafe {
        NtCreateFile(
            &raw mut opened,
            desired_access(expected, access),
            &raw mut attributes,
            &raw mut io_status,
            std::ptr::null_mut(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_OPEN,
            options,
            std::ptr::null_mut(),
            0,
        )
    };
    if status < 0 {
        Err(io::Error::other(format!(
            "relative filesystem open failed with NTSTATUS 0x{:08X}",
            status as u32
        )))
    } else {
        Ok(unsafe { File::from_raw_handle(opened) })
    }
}

fn desired_access(expected: ExistingEntryType, access: ExistingEntryAccess) -> u32 {
    let security = match access {
        ExistingEntryAccess::ReadOnly => 0,
        ExistingEntryAccess::SecurityDescriptor => {
            windows_sys::Win32::Storage::FileSystem::READ_CONTROL
                | windows_sys::Win32::Storage::FileSystem::WRITE_DAC
        }
    };
    (match expected {
        ExistingEntryType::File => FILE_READ_DATA,
        ExistingEntryType::Directory => FILE_LIST_DIRECTORY,
    }) | FILE_READ_ATTRIBUTES
        | SYNCHRONIZE_ACCESS
        | security
}

fn reject_nul(value: &OsStr) -> io::Result<()> {
    if value.encode_wide().any(|unit| unit == 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains NUL",
        ));
    }
    Ok(())
}
