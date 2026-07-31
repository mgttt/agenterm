use std::{fs::Metadata, io, path::Path};

pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    std::fs::rename(source, destination)
}

pub(crate) fn sync_parent(parent: &Path) -> io::Result<()> {
    std::fs::File::open(parent)?.sync_all()
}

pub(crate) fn metadata_is_reparse_point(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}
