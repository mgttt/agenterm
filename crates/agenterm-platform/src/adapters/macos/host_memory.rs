use crate::contract::host_memory::{
    HostMemoryError, HostMemoryErrorKind, HostMemoryFacts, checked_facts,
};

pub(crate) fn facts() -> Result<HostMemoryFacts, HostMemoryError> {
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let page_size = u64::try_from(page_size).map_err(|_| {
        HostMemoryError::new(
            HostMemoryErrorKind::InvalidValue,
            format!("host reported invalid page size: {page_size}"),
        )
    })?;

    let mut physical_bytes = 0_u64;
    let mut length = std::mem::size_of::<u64>();
    let result = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&raw mut physical_bytes).cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(HostMemoryError::new(
            HostMemoryErrorKind::Query,
            std::io::Error::last_os_error().to_string(),
        ));
    }
    if length != std::mem::size_of::<u64>() {
        return Err(HostMemoryError::new(
            HostMemoryErrorKind::InvalidValue,
            format!("hw.memsize returned {length} bytes"),
        ));
    }
    checked_facts(page_size, page_size, physical_bytes)
}
