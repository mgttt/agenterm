//! Script Runtime filesystem primitives whose durability mechanics are OS-specific.

use std::{fs::Metadata, io, path::Path};

pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    agenterm_platform::filesystem::replace_file(source, destination)
}

pub(crate) fn sync_parent(parent: &Path) -> io::Result<()> {
    agenterm_platform::filesystem::sync_parent(parent)
}

pub(crate) fn metadata_is_reparse_point(metadata: &Metadata) -> bool {
    agenterm_platform::filesystem::metadata_is_link_like(metadata)
}
