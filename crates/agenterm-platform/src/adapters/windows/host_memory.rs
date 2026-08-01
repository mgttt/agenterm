use std::num::{NonZeroU64, NonZeroUsize};

use crate::contract::host_memory::{HostMemoryError, HostMemoryErrorKind, HostMemoryFacts};

pub(crate) fn facts() -> Result<HostMemoryFacts, HostMemoryError> {
    use windows_sys::Win32::System::SystemInformation::{
        GetSystemInfo, GlobalMemoryStatusEx, MEMORYSTATUSEX, SYSTEM_INFO,
    };

    let mut system = SYSTEM_INFO::default();
    unsafe { GetSystemInfo(&mut system) };
    let page_size = nonzero_usize("page size", system.dwPageSize)?;
    let allocation_granularity =
        nonzero_usize("allocation granularity", system.dwAllocationGranularity)?;

    let mut memory = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    if unsafe { GlobalMemoryStatusEx(&mut memory) } == 0 {
        return Err(HostMemoryError::new(
            HostMemoryErrorKind::Query,
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let physical_bytes = NonZeroU64::new(memory.ullTotalPhys).ok_or_else(|| {
        HostMemoryError::new(
            HostMemoryErrorKind::InvalidValue,
            "host reported zero physical memory",
        )
    })?;
    Ok(HostMemoryFacts {
        page_size,
        allocation_granularity,
        physical_bytes,
    })
}

fn nonzero_usize(name: &str, value: u32) -> Result<NonZeroUsize, HostMemoryError> {
    NonZeroUsize::new(value as usize).ok_or_else(|| {
        HostMemoryError::new(
            HostMemoryErrorKind::InvalidValue,
            format!("host reported zero {name}"),
        )
    })
}
