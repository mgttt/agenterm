use std::{fs::Metadata, io, path::Path};

pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = std::fs::canonicalize(source)?;
    let destination = std::fs::canonicalize(
        destination
            .parent()
            .ok_or_else(|| io::Error::other("destination parent required"))?,
    )?
    .join(
        destination
            .file_name()
            .ok_or_else(|| io::Error::other("destination name required"))?,
    );
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(crate) fn sync_parent(_parent: &Path) -> io::Result<()> {
    Ok(())
}

pub(crate) fn metadata_is_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    attributes_are_reparse(metadata.file_attributes())
}

fn attributes_are_reparse(attributes: u32) -> bool {
    attributes & 0x0000_0400 != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reparse_attribute_detection_is_exact() {
        assert!(!attributes_are_reparse(0));
        assert!(attributes_are_reparse(0x0000_0400));
        assert!(attributes_are_reparse(0x0000_0400 | 0x10));
    }
}
