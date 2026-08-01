use crate::contract::host_memory::{
    HostMemoryAvailability, HostMemoryAvailabilitySemantics, HostMemoryError, HostMemoryErrorKind,
    HostMemoryFacts, checked_availability, checked_facts,
};
use windows_sys::Win32::System::SystemInformation::MEMORYSTATUSEX;

pub(crate) fn facts() -> Result<HostMemoryFacts, HostMemoryError> {
    use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};

    let mut system = SYSTEM_INFO::default();
    unsafe { GetSystemInfo(&mut system) };
    let memory = memory_status()?;
    checked_facts(
        u64::from(system.dwPageSize),
        u64::from(system.dwAllocationGranularity),
        memory.ullTotalPhys,
    )
}

pub(crate) fn availability() -> Result<HostMemoryAvailability, HostMemoryError> {
    let memory = memory_status()?;
    checked_availability(
        memory.ullAvailPhys,
        memory.ullTotalPhys,
        HostMemoryAvailabilitySemantics::WindowsAvailablePhysical,
    )
}

fn memory_status() -> Result<MEMORYSTATUSEX, HostMemoryError> {
    use windows_sys::Win32::System::SystemInformation::GlobalMemoryStatusEx;

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
    Ok(memory)
}
