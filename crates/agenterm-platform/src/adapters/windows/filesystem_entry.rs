//! Windows filesystem entry classification.

use std::{fs::Metadata, os::windows::fs::MetadataExt as _};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

pub(crate) fn metadata_is_link_like(metadata: &Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}
