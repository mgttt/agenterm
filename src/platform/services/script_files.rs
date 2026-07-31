//! Script Runtime filesystem primitives whose durability mechanics are OS-specific.

use std::{fs::Metadata, io, path::Path};

use crate::platform::selected;

pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    selected::script_files::replace_file(source, destination)
}

pub(crate) fn sync_parent(parent: &Path) -> io::Result<()> {
    selected::script_files::sync_parent(parent)
}

pub(crate) fn metadata_is_reparse_point(metadata: &Metadata) -> bool {
    selected::script_files::metadata_is_reparse_point(metadata)
}
