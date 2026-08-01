use std::os::windows::ffi::OsStrExt;

use crate::contract::storage::{
    StorageError, StorageErrorKind, VolumeSpace, checked_product, checked_space,
};

pub(crate) fn volume_space(path: &std::path::Path) -> Result<VolumeSpace, StorageError> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDiskFreeSpaceW, GetVolumePathNameW,
    };

    let path = wide(path)?;
    let mut root = vec![0_u16; 32_768];
    if unsafe { GetVolumePathNameW(path.as_ptr(), root.as_mut_ptr(), root.len() as u32) } == 0 {
        return Err(query_error("resolve volume root"));
    }

    let mut available = 0_u64;
    let mut total = 0_u64;
    let mut free = 0_u64;
    if unsafe { GetDiskFreeSpaceExW(root.as_ptr(), &mut available, &mut total, &mut free) } == 0 {
        return Err(query_error("query volume capacity"));
    }

    let mut sectors_per_cluster = 0_u32;
    let mut bytes_per_sector = 0_u32;
    let mut free_clusters = 0_u32;
    let mut total_clusters = 0_u32;
    if unsafe {
        GetDiskFreeSpaceW(
            root.as_ptr(),
            &mut sectors_per_cluster,
            &mut bytes_per_sector,
            &mut free_clusters,
            &mut total_clusters,
        )
    } == 0
    {
        return Err(query_error("query volume allocation unit"));
    }
    let allocation_unit = checked_product(
        u64::from(sectors_per_cluster),
        u64::from(bytes_per_sector),
        "sectors per cluster",
    )?;
    checked_space(total, available, allocation_unit)
}

fn wide(path: &std::path::Path) -> Result<Vec<u16>, StorageError> {
    let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
    if value.contains(&0) {
        return Err(StorageError::new(
            StorageErrorKind::Path,
            "path contains an embedded NUL",
        ));
    }
    value.push(0);
    Ok(value)
}

fn query_error(operation: &str) -> StorageError {
    StorageError::new(
        StorageErrorKind::Query,
        format!("{operation}: {}", std::io::Error::last_os_error()),
    )
}
