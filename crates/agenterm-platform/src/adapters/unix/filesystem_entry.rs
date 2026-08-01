//! Unix filesystem entry classification.

use std::fs::Metadata;

pub(crate) fn metadata_is_link_like(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}
