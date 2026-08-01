use std::ffi::CStr;

use crate::contract::cache_hierarchy::{
    CacheGeometryFacts, CacheHierarchyError, CacheHierarchyErrorKind, CacheHierarchyFacts,
    CacheKind,
};

pub(crate) fn facts() -> Result<CacheHierarchyFacts, CacheHierarchyError> {
    let line_bytes = sysctl_integer(c"hw.cachelinesize").map_err(|error| {
        CacheHierarchyError::new(
            CacheHierarchyErrorKind::Query,
            format!("hw.cachelinesize: {error}"),
        )
    })?;
    let mut geometries = Vec::new();
    for (name, level, kind) in [
        (c"hw.l1dcachesize", 1, CacheKind::Data),
        (c"hw.l1icachesize", 1, CacheKind::Instruction),
        (c"hw.l2cachesize", 2, CacheKind::Unified),
        (c"hw.l3cachesize", 3, CacheKind::Unified),
    ] {
        let Some(size_bytes) = optional_sysctl_integer(name)? else {
            continue;
        };
        geometries.push(CacheGeometryFacts::from_raw(
            level, kind, size_bytes, line_bytes, None, None,
        )?);
    }
    CacheHierarchyFacts::new(geometries)
}

fn optional_sysctl_integer(name: &CStr) -> Result<Option<u64>, CacheHierarchyError> {
    match sysctl_integer(name) {
        Ok(0) => Ok(None),
        Ok(value) => Ok(Some(value)),
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => Ok(None),
        Err(error) => Err(CacheHierarchyError::new(
            CacheHierarchyErrorKind::Query,
            format!("{}: {error}", name.to_string_lossy()),
        )),
    }
}

fn sysctl_integer(name: &CStr) -> std::io::Result<u64> {
    let mut length = 0_usize;
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    if !matches!(length, 4 | 8) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} returned {length} bytes", name.to_string_lossy()),
        ));
    }
    let expected = length;
    let mut bytes = [0_u8; 8];
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    if length != expected {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} changed size to {length} bytes", name.to_string_lossy()),
        ));
    }
    Ok(match length {
        4 => u64::from(u32::from_ne_bytes(bytes[..4].try_into().unwrap())),
        8 => u64::from_ne_bytes(bytes),
        _ => unreachable!(),
    })
}
