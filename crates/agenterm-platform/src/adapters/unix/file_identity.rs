//! POSIX opened-file identity shared by Linux and macOS adapters.

use std::os::unix::fs::MetadataExt as _;

use crate::filesystem::FileIdentity;

pub fn file_identity(file: &std::fs::File) -> std::io::Result<FileIdentity> {
    let metadata = file.metadata()?;
    Ok(FileIdentity {
        filesystem_id: metadata.dev(),
        object_id: metadata.ino(),
        hard_link_count: metadata.nlink(),
    })
}

pub fn path_identity(path: &std::path::Path) -> std::io::Result<FileIdentity> {
    file_identity(&std::fs::File::open(path)?)
}
