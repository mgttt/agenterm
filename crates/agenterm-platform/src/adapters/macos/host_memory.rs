use crate::contract::host_memory::{
    HostMemoryAvailability, HostMemoryAvailabilitySemantics, HostMemoryError, HostMemoryErrorKind,
    HostMemoryFacts, checked_availability, checked_facts,
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

pub(crate) fn availability() -> Result<HostMemoryAvailability, HostMemoryError> {
    unsafe extern "C" {
        fn mach_host_self() -> libc::mach_port_t;
    }

    let mut statistics = unsafe { std::mem::zeroed::<libc::vm_statistics64_data_t>() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    let result = unsafe {
        libc::host_statistics64(
            mach_host_self(),
            libc::HOST_VM_INFO64,
            (&raw mut statistics).cast(),
            &mut count,
        )
    };
    if result != libc::KERN_SUCCESS {
        return Err(HostMemoryError::new(
            HostMemoryErrorKind::Query,
            format!("host_statistics64 returned Mach status {result}"),
        ));
    }
    if count < libc::HOST_VM_INFO64_COUNT {
        return Err(HostMemoryError::new(
            HostMemoryErrorKind::InvalidValue,
            format!(
                "host_statistics64 returned {count} integers, expected {}",
                libc::HOST_VM_INFO64_COUNT
            ),
        ));
    }
    let available_pages = u64::from(statistics.free_count)
        .checked_add(u64::from(statistics.inactive_count))
        .ok_or_else(|| {
            HostMemoryError::new(
                HostMemoryErrorKind::Overflow,
                "Mach free plus inactive page count overflowed u64",
            )
        })?;
    let memory = facts()?;
    let available_physical_bytes = available_pages
        .checked_mul(memory.page_size.get() as u64)
        .ok_or_else(|| {
            HostMemoryError::new(
                HostMemoryErrorKind::Overflow,
                "Mach available page count multiplied by page size overflowed u64",
            )
        })?;
    checked_availability(
        available_physical_bytes,
        memory.physical_bytes.get(),
        HostMemoryAvailabilitySemantics::MacosFreeAndInactive,
    )
}
