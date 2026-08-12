//! Unix atomic file publication adapter.

use std::path::Path;

pub fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

pub fn sync_parent(parent: &Path) -> std::io::Result<()> {
    std::fs::File::open(parent)?.sync_all()
}

pub fn normalize_owned_destination(destination: &Path) -> std::io::Result<std::path::PathBuf> {
    let name = destination
        .file_name()
        .ok_or_else(|| std::io::Error::other("destination file name required"))?;
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::canonicalize(parent).map(|parent| parent.join(name))
}
