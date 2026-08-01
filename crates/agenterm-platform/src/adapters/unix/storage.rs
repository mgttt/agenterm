use std::os::unix::ffi::OsStrExt;

use crate::contract::storage::{
    StorageError, StorageErrorKind, VolumeSpace, checked_product, checked_space,
};

pub(crate) fn volume_space(path: &std::path::Path) -> Result<VolumeSpace, StorageError> {
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| StorageError::new(StorageErrorKind::Path, "path contains an embedded NUL"))?;
    let mut facts = unsafe { std::mem::zeroed::<libc::statvfs>() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut facts) } != 0 {
        return Err(StorageError::new(
            StorageErrorKind::Query,
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let allocation_unit = if facts.f_frsize == 0 {
        facts.f_bsize
    } else {
        facts.f_frsize
    };
    let total = checked_product(facts.f_blocks, allocation_unit, "total blocks")?;
    let available = checked_product(facts.f_bavail, allocation_unit, "available blocks")?;
    checked_space(total, available, allocation_unit)
}
